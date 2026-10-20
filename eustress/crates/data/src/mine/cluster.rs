//! Clustering: k-means with k-means++ seeding, density-based (DBSCAN),
//! hierarchical (agglomerative), and the internal indices that score a
//! partition.
//!
//! k-means finds round, similar-sized clusters. The other two families cover
//! what it cannot: DBSCAN finds clusters of any shape and marks outliers as
//! noise instead of forcing them into a cluster, and agglomerative clustering
//! builds the full merge hierarchy, so the number of clusters is a cut rather
//! than a guess made up front.
//!
//! No clustering has an objective ground truth (no method can be scale
//! invariant, rich, and consistent at once), which is why the internal indices
//! below exist: they are how two partitions of the same data get compared.

use serde::Serialize;

use super::{dist2, Matrix, Rng};
use crate::{DataError, Result};

// ─────────────────────────────────────────────────────────────────────────────
// DBSCAN
// ─────────────────────────────────────────────────────────────────────────────

/// Density-based clustering: a cluster is a region where points have at least
/// `min_samples` neighbours (themselves included) within `eps`.
///
/// `eps` is in the units of the features, so standardise first when they are
/// measured on different scales. Brute-force neighbour search, O(n²).
#[derive(Clone, Debug, PartialEq)]
pub struct Dbscan {
    pub eps: f64,
    pub min_samples: usize,
}

impl Dbscan {
    /// Cluster index per row, `None` for noise. Clusters are numbered in the
    /// order their first core point appears, so the result is deterministic.
    pub fn fit(&self, x: &Matrix) -> Result<Vec<Option<usize>>> {
        if !(self.eps > 0.0) {
            return Err(DataError::Schema("DBSCAN eps must be positive".into()));
        }
        if self.min_samples == 0 {
            return Err(DataError::Schema("DBSCAN min_samples must be at least 1".into()));
        }
        let n = x.rows();
        let eps2 = self.eps * self.eps;
        let region = |p: usize| -> Vec<usize> {
            (0..n).filter(|&q| dist2(x.row(p), x.row(q)) <= eps2).collect()
        };
        const UNSEEN: usize = usize::MAX;
        const NOISE: usize = usize::MAX - 1;
        let mut label = vec![UNSEEN; n];
        let mut next = 0;
        for p in 0..n {
            if label[p] != UNSEEN {
                continue;
            }
            let seeds = region(p);
            if seeds.len() < self.min_samples {
                label[p] = NOISE;
                continue;
            }
            label[p] = next;
            let mut queue: std::collections::VecDeque<usize> = seeds.into_iter().collect();
            while let Some(q) = queue.pop_front() {
                if label[q] == NOISE {
                    // A border point: reachable from a core point, so it joins,
                    // but it is not dense enough to extend the cluster.
                    label[q] = next;
                }
                if label[q] != UNSEEN {
                    continue;
                }
                label[q] = next;
                let nq = region(q);
                if nq.len() >= self.min_samples {
                    queue.extend(nq);
                }
            }
            next += 1;
        }
        Ok(label.into_iter().map(|l| if l == NOISE { None } else { Some(l) }).collect())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// k-means with k-means++ seeding
// ─────────────────────────────────────────────────────────────────────────────

/// k-means (Lloyd's algorithm) with k-means++ seeding and seeded restarts.
///
/// k-means++ picks each starting centre with probability proportional to its
/// squared distance from the centres already chosen, which spreads the start
/// across the data and keeps the expected result within O(log k) of the
/// optimum. Lloyd's algorithm still stops at a local optimum, so the run
/// repeats `n_init` times and keeps the partition with the lowest inertia.
/// [`crate::ml::kmeans`] is the seedless floor that starts from evenly spaced
/// rows, which is fragile when the rows arrive sorted.
#[derive(Clone, Debug, PartialEq)]
pub struct KMeans {
    pub k: usize,
    /// Independent seeded restarts; the lowest inertia wins.
    pub n_init: usize,
    pub max_iter: usize,
    pub seed: u64,
}

impl Default for KMeans {
    fn default() -> Self {
        Self { k: 8, n_init: 10, max_iter: 300, seed: 0 }
    }
}

/// A fitted k-means partition.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct KMeansModel {
    /// One centre per non-empty cluster, in the units of the input.
    pub centroids: Vec<Vec<f64>>,
    /// Cluster per row, numbered by first appearance: row 0 is always in
    /// cluster 0, so equal partitions always carry equal labels.
    pub labels: Vec<usize>,
    /// Sum of squared distances from each row to its centre.
    pub inertia: f64,
    /// Lloyd iterations of the winning restart.
    pub iterations: usize,
}

/// k-means++ starting centres.
fn plus_plus(x: &Matrix, k: usize, rng: &mut Rng) -> Vec<Vec<f64>> {
    let n = x.rows();
    let mut centres = vec![x.row(rng.below(n)).to_vec()];
    let mut d2: Vec<f64> = (0..n).map(|i| dist2(x.row(i), &centres[0])).collect();
    while centres.len() < k {
        let total: f64 = d2.iter().sum();
        let next = if total > 0.0 {
            let mut t = rng.next_f64() * total;
            let mut pick = n - 1;
            for (i, &w) in d2.iter().enumerate() {
                if t < w {
                    pick = i;
                    break;
                }
                t -= w;
            }
            pick
        } else {
            // Every row sits on a chosen centre: fewer distinct rows than k.
            rng.below(n)
        };
        let c = x.row(next).to_vec();
        for (i, d) in d2.iter_mut().enumerate() {
            *d = d.min(dist2(x.row(i), &c));
        }
        centres.push(c);
    }
    centres
}

impl KMeans {
    pub fn fit(&self, x: &Matrix) -> Result<KMeansModel> {
        let n = x.rows();
        if self.k == 0 || self.k > n {
            return Err(DataError::Schema(format!(
                "k-means needs 1 <= k <= rows, got k = {} for {n} rows",
                self.k
            )));
        }
        if self.n_init == 0 {
            return Err(DataError::Schema("k-means needs at least one restart".into()));
        }
        let mut rng = Rng::new(self.seed);
        let mut best: Option<KMeansModel> = None;
        for _ in 0..self.n_init {
            let run = self.lloyd(x, plus_plus(x, self.k, &mut rng));
            if best.as_ref().is_none_or(|b| run.inertia < b.inertia) {
                best = Some(run);
            }
        }
        best.ok_or_else(|| DataError::Schema("k-means produced no partition".into()))
    }

    fn lloyd(&self, x: &Matrix, mut centroids: Vec<Vec<f64>>) -> KMeansModel {
        let (n, d, k) = (x.rows(), x.cols(), centroids.len());
        let mut labels = vec![usize::MAX; n];
        let mut iterations = 0;
        while iterations < self.max_iter.max(1) {
            iterations += 1;
            let mut changed = false;
            for (i, label) in labels.iter_mut().enumerate() {
                let row = x.row(i);
                let (mut best, mut best_d) = (0, f64::INFINITY);
                for (c, centre) in centroids.iter().enumerate() {
                    let dd = dist2(row, centre);
                    if dd < best_d {
                        best_d = dd;
                        best = c;
                    }
                }
                if *label != best {
                    *label = best;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
            let mut sums = vec![vec![0.0; d]; k];
            let mut counts = vec![0usize; k];
            for (i, &l) in labels.iter().enumerate() {
                counts[l] += 1;
                for (s, v) in sums[l].iter_mut().zip(x.row(i)) {
                    *s += v;
                }
            }
            // An emptied cluster keeps its old centre.
            for c in 0..k {
                if counts[c] > 0 {
                    for j in 0..d {
                        centroids[c][j] = sums[c][j] / counts[c] as f64;
                    }
                }
            }
        }
        let inertia = labels.iter().enumerate().map(|(i, &l)| dist2(x.row(i), &centroids[l])).sum();
        // Renumber by first appearance and drop empty clusters.
        let mut renumber = vec![usize::MAX; k];
        let mut ordered = Vec::new();
        for l in labels.iter_mut() {
            if renumber[*l] == usize::MAX {
                renumber[*l] = ordered.len();
                ordered.push(centroids[*l].clone());
            }
            *l = renumber[*l];
        }
        KMeansModel { centroids: ordered, labels, inertia, iterations }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Agglomerative
// ─────────────────────────────────────────────────────────────────────────────

/// How the distance between two clusters is defined.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Linkage {
    /// Closest pair. Finds elongated clusters; prone to chaining.
    Single,
    /// Farthest pair. Compact clusters of similar diameter.
    Complete,
    /// Mean of all pairs (UPGMA).
    Average,
    /// Smallest increase in within-cluster variance. Round clusters; usually
    /// the best default.
    Ward,
}

/// Bottom-up hierarchical clustering, cut at `n_clusters`.
///
/// Single linkage runs as a minimum spanning tree (O(n²) time, O(n) memory).
/// The other linkages use the nearest-neighbour chain algorithm with
/// Lance-Williams updates (O(n²) time and memory), which is exact for them
/// because they are reducible. The distance matrix caps the input at
/// [`Agglomerative::MAX_ROWS`] rows; sample larger data.
#[derive(Clone, Debug, PartialEq)]
pub struct Agglomerative {
    pub n_clusters: usize,
    pub linkage: Linkage,
}

struct UnionFind(Vec<usize>);

impl UnionFind {
    fn new(n: usize) -> Self {
        Self((0..n).collect())
    }
    fn find(&mut self, mut a: usize) -> usize {
        while self.0[a] != a {
            self.0[a] = self.0[self.0[a]];
            a = self.0[a];
        }
        a
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
            self.0[hi] = lo;
        }
    }
}

/// Index of pair `(i, j)`, `i < j`, in a condensed upper triangle over `n`.
fn tri(i: usize, j: usize, n: usize) -> usize {
    debug_assert!(i < j);
    i * n - i * (i + 1) / 2 + (j - i - 1)
}

impl Agglomerative {
    /// Largest input the O(n²) distance matrix is allowed to cover
    /// (about 100 MB of distances).
    pub const MAX_ROWS: usize = 5000;

    /// Cluster index per row, numbered by each cluster's lowest row.
    pub fn fit(&self, x: &Matrix) -> Result<Vec<usize>> {
        let n = x.rows();
        if self.n_clusters == 0 || self.n_clusters > n {
            return Err(DataError::Schema(format!(
                "cannot cut {n} rows into {} clusters",
                self.n_clusters
            )));
        }
        if n > Self::MAX_ROWS {
            return Err(DataError::Schema(format!(
                "agglomerative clustering holds an n x n distance matrix; {n} rows exceeds the \
                 {} row limit, so cluster a sample",
                Self::MAX_ROWS
            )));
        }
        // Each merge is (height, discovery order, a, b).
        let mut merges: Vec<(f64, usize, usize, usize)> = match self.linkage {
            Linkage::Single => single_linkage_mst(x),
            other => nn_chain(x, other),
        };
        merges.sort_by(|p, q| p.0.total_cmp(&q.0).then(p.1.cmp(&q.1)));
        // The n - k lowest merges ARE the cut: union-find makes the result
        // independent of the order they are applied in.
        let mut uf = UnionFind::new(n);
        for &(_, _, a, b) in merges.iter().take(n - self.n_clusters) {
            uf.union(a, b);
        }
        let mut id_of_root = vec![usize::MAX; n];
        let mut next = 0;
        let mut labels = vec![0; n];
        for (i, slot) in labels.iter_mut().enumerate() {
            let r = uf.find(i);
            if id_of_root[r] == usize::MAX {
                id_of_root[r] = next;
                next += 1;
            }
            *slot = id_of_root[r];
        }
        Ok(labels)
    }
}

/// Prim's algorithm on the complete graph: the MST's edges are exactly the
/// single-linkage merges.
fn single_linkage_mst(x: &Matrix) -> Vec<(f64, usize, usize, usize)> {
    let n = x.rows();
    if n < 2 {
        return Vec::new();
    }
    let mut in_tree = vec![false; n];
    let mut best = vec![f64::INFINITY; n];
    let mut parent = vec![0usize; n];
    in_tree[0] = true;
    for v in 1..n {
        best[v] = dist2(x.row(0), x.row(v));
    }
    let mut edges = Vec::with_capacity(n - 1);
    for order in 0..n - 1 {
        let mut u = usize::MAX;
        for v in 0..n {
            if !in_tree[v] && (u == usize::MAX || best[v] < best[u]) {
                u = v;
            }
        }
        in_tree[u] = true;
        edges.push((best[u].sqrt(), order, parent[u], u));
        for v in 0..n {
            if !in_tree[v] {
                let d = dist2(x.row(u), x.row(v));
                if d < best[v] {
                    best[v] = d;
                    parent[v] = u;
                }
            }
        }
    }
    edges
}

/// Nearest-neighbour chain with Lance-Williams updates, for complete, average
/// and Ward linkage. Ward works on squared distances, as its recurrence
/// requires; the others on plain distances.
fn nn_chain(x: &Matrix, linkage: Linkage) -> Vec<(f64, usize, usize, usize)> {
    let n = x.rows();
    let mut merges = Vec::with_capacity(n.saturating_sub(1));
    if n < 2 {
        return merges;
    }
    let mut d = vec![0.0; n * (n - 1) / 2];
    for i in 0..n {
        for j in i + 1..n {
            let d2 = dist2(x.row(i), x.row(j));
            d[tri(i, j, n)] = if linkage == Linkage::Ward { d2 } else { d2.sqrt() };
        }
    }
    let get = |d: &[f64], a: usize, b: usize| if a < b { d[tri(a, b, n)] } else { d[tri(b, a, n)] };
    let mut size = vec![1usize; n];
    let mut active = vec![true; n];
    let mut chain: Vec<usize> = Vec::new();
    let mut remaining = n;
    while remaining > 1 {
        if chain.is_empty() {
            chain.push((0..n).find(|&i| active[i]).expect("an active cluster remains"));
        }
        let (a, b) = loop {
            let a = *chain.last().expect("chain is not empty");
            let prev = if chain.len() >= 2 { Some(chain[chain.len() - 2]) } else { None };
            // Nearest active neighbour of `a`. Preferring the previous chain
            // element on a tie guarantees the chain terminates.
            let mut best: Option<(f64, usize)> = None;
            for c in 0..n {
                if c == a || !active[c] {
                    continue;
                }
                let dc = get(&d, a, c);
                let better = match best {
                    None => true,
                    Some((bd, bc)) => dc < bd || (dc == bd && Some(c) == prev && Some(bc) != prev),
                };
                if better {
                    best = Some((dc, c));
                }
            }
            let (_, nb) = best.expect("at least two clusters are active");
            if Some(nb) == prev {
                chain.pop();
                chain.pop();
                break (a, nb);
            }
            chain.push(nb);
        };
        let height = get(&d, a, b);
        let height = if linkage == Linkage::Ward { height.sqrt() } else { height };
        merges.push((height, merges.len(), a, b));
        // Merge b into a; update a's distance to every other active cluster.
        let (na, nb) = (size[a] as f64, size[b] as f64);
        let dab = get(&d, a, b);
        for k in 0..n {
            if k == a || k == b || !active[k] {
                continue;
            }
            let (dak, dbk) = (get(&d, a, k), get(&d, b, k));
            let nk = size[k] as f64;
            let v = match linkage {
                Linkage::Complete => dak.max(dbk),
                Linkage::Average => (na * dak + nb * dbk) / (na + nb),
                Linkage::Ward => ((na + nk) * dak + (nb + nk) * dbk - nk * dab) / (na + nb + nk),
                Linkage::Single => dak.min(dbk),
            };
            let idx = if a < k { tri(a, k, n) } else { tri(k, a, n) };
            d[idx] = v;
        }
        active[b] = false;
        size[a] += size[b];
        remaining -= 1;
    }
    merges
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal indices
// ─────────────────────────────────────────────────────────────────────────────

fn check_partition(x: &Matrix, labels: &[usize]) -> Result<usize> {
    if x.rows() != labels.len() {
        return Err(DataError::Schema(format!(
            "{} rows but {} labels",
            x.rows(),
            labels.len()
        )));
    }
    let k = labels.iter().max().map_or(0, |m| m + 1);
    let present = {
        let mut seen = vec![false; k];
        labels.iter().for_each(|&l| seen[l] = true);
        seen.into_iter().filter(|&s| s).count()
    };
    if present < 2 || present >= x.rows() {
        return Err(DataError::Schema(format!(
            "an internal index needs between 2 and rows - 1 clusters, got {present}"
        )));
    }
    Ok(k)
}

fn centroids(x: &Matrix, labels: &[usize], k: usize) -> (Vec<Vec<f64>>, Vec<usize>) {
    let mut c = vec![vec![0.0; x.cols()]; k];
    let mut count = vec![0usize; k];
    for i in 0..x.rows() {
        count[labels[i]] += 1;
        for (s, v) in c[labels[i]].iter_mut().zip(x.row(i)) {
            *s += v;
        }
    }
    for (ci, &m) in c.iter_mut().zip(&count) {
        if m > 0 {
            for s in ci.iter_mut() {
                *s /= m as f64;
            }
        }
    }
    (c, count)
}

/// Mean silhouette in `[-1, 1]`: how much closer each point is to its own
/// cluster than to the nearest other one. Higher is better; a point alone in
/// its cluster scores 0. O(n²).
pub fn silhouette(x: &Matrix, labels: &[usize]) -> Result<f64> {
    let k = check_partition(x, labels)?;
    let n = x.rows();
    let (_, count) = centroids(x, labels, k);
    let mut total = 0.0;
    let mut sums = vec![0.0; k];
    for i in 0..n {
        sums.iter_mut().for_each(|s| *s = 0.0);
        for j in 0..n {
            if i != j {
                sums[labels[j]] += dist2(x.row(i), x.row(j)).sqrt();
            }
        }
        let own = labels[i];
        if count[own] <= 1 {
            continue;
        }
        let a = sums[own] / (count[own] - 1) as f64;
        let b = (0..k)
            .filter(|&c| c != own && count[c] > 0)
            .map(|c| sums[c] / count[c] as f64)
            .fold(f64::INFINITY, f64::min);
        let m = a.max(b);
        if m > 0.0 {
            total += (b - a) / m;
        }
    }
    Ok(total / n as f64)
}

/// Davies-Bouldin index: average over clusters of the worst ratio of combined
/// spread to centroid separation. Lower is better; 0 is the ideal.
pub fn davies_bouldin(x: &Matrix, labels: &[usize]) -> Result<f64> {
    let k = check_partition(x, labels)?;
    let (c, count) = centroids(x, labels, k);
    let mut spread = vec![0.0; k];
    for i in 0..x.rows() {
        spread[labels[i]] += dist2(x.row(i), &c[labels[i]]).sqrt();
    }
    let live: Vec<usize> = (0..k).filter(|&i| count[i] > 0).collect();
    for &i in &live {
        spread[i] /= count[i] as f64;
    }
    let mut total = 0.0;
    for &i in &live {
        let mut worst = 0.0f64;
        for &j in &live {
            if i == j {
                continue;
            }
            let sep = dist2(&c[i], &c[j]).sqrt();
            if sep == 0.0 {
                return Err(DataError::Schema(format!(
                    "clusters {i} and {j} share a centroid, so their separation is zero"
                )));
            }
            worst = worst.max((spread[i] + spread[j]) / sep);
        }
        total += worst;
    }
    Ok(total / live.len() as f64)
}

/// Calinski-Harabasz index: between-cluster dispersion over within-cluster
/// dispersion, each per degree of freedom. Higher is better; infinite for a
/// partition whose clusters have no internal spread.
pub fn calinski_harabasz(x: &Matrix, labels: &[usize]) -> Result<f64> {
    let k = check_partition(x, labels)?;
    let n = x.rows();
    let (c, count) = centroids(x, labels, k);
    let live = count.iter().filter(|&&m| m > 0).count();
    let mean: Vec<f64> = (0..x.cols()).map(|j| x.column(j).iter().sum::<f64>() / n as f64).collect();
    let between: f64 = (0..k).map(|i| count[i] as f64 * dist2(&c[i], &mean)).sum();
    let within: f64 = (0..n).map(|i| dist2(x.row(i), &c[labels[i]])).sum();
    if within == 0.0 {
        return Ok(f64::INFINITY);
    }
    Ok((between / (live - 1) as f64) / (within / (n - live) as f64))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two tight blobs far apart, plus one far outlier (row 10).
    fn blobs_with_outlier() -> Matrix {
        let mut rows = Vec::new();
        for i in 0..5 {
            rows.push(vec![i as f64 * 0.1, 0.0]);
        }
        for i in 0..5 {
            rows.push(vec![10.0 + i as f64 * 0.1, 10.0]);
        }
        rows.push(vec![50.0, -50.0]);
        Matrix::from_rows(&rows).unwrap()
    }

    /// Three 3 x 3 grids of points, 50 apart, with the rows sorted by blob.
    fn three_blobs() -> Matrix {
        let mut rows = Vec::new();
        for (cx, cy) in [(0.0, 0.0), (50.0, 0.0), (0.0, 50.0)] {
            for i in 0..9 {
                rows.push(vec![cx + (i % 3) as f64 * 0.5, cy + (i / 3) as f64 * 0.5]);
            }
        }
        Matrix::from_rows(&rows).unwrap()
    }

    #[test]
    fn kmeans_finds_three_separated_blobs() {
        let m = KMeans { k: 3, ..Default::default() }.fit(&three_blobs()).unwrap();
        for b in 0..3 {
            assert!(m.labels[b * 9..(b + 1) * 9].iter().all(|&l| l == b), "blob {b}: {:?}", m.labels);
        }
        // Each centre sits on its blob's middle point.
        assert!((m.centroids[1][0] - 50.5).abs() < 1e-9 && (m.centroids[1][1] - 0.5).abs() < 1e-9);
        // Per blob, x and y offsets from the centre are each -0.5, 0, 0.5 three
        // times over: 1.5 + 1.5 = 3 of squared distance, 9 across three blobs.
        assert!((m.inertia - 9.0).abs() < 1e-9, "{}", m.inertia);
    }

    #[test]
    fn kmeans_is_reproducible_by_seed() {
        let x = blobs_with_outlier();
        let a = KMeans { k: 2, seed: 9, ..Default::default() }.fit(&x).unwrap();
        let b = KMeans { k: 2, seed: 9, ..Default::default() }.fit(&x).unwrap();
        assert_eq!(a, b);
        assert!(KMeans { k: 12, ..Default::default() }.fit(&x).is_err(), "more clusters than rows");
    }

    #[test]
    fn dbscan_finds_the_blobs_and_leaves_the_outlier_as_noise() {
        let labels = Dbscan { eps: 0.5, min_samples: 3 }.fit(&blobs_with_outlier()).unwrap();
        assert!(labels[..5].iter().all(|&l| l == Some(0)), "{labels:?}");
        assert!(labels[5..10].iter().all(|&l| l == Some(1)), "{labels:?}");
        assert_eq!(labels[10], None, "the outlier belongs to no cluster");
    }

    #[test]
    fn dbscan_follows_a_chain_of_any_shape() {
        // An L-shaped path of evenly spaced points is one cluster, not two.
        let mut rows: Vec<Vec<f64>> = (0..10).map(|i| vec![i as f64, 0.0]).collect();
        rows.extend((1..10).map(|i| vec![9.0, i as f64]));
        let labels = Dbscan { eps: 1.01, min_samples: 2 }
            .fit(&Matrix::from_rows(&rows).unwrap())
            .unwrap();
        assert!(labels.iter().all(|&l| l == Some(0)), "{labels:?}");
    }

    #[test]
    fn every_linkage_separates_two_distant_blobs() {
        let x = blobs_with_outlier().select_rows(&(0..10).collect::<Vec<_>>());
        for linkage in [Linkage::Single, Linkage::Complete, Linkage::Average, Linkage::Ward] {
            let l = Agglomerative { n_clusters: 2, linkage }.fit(&x).unwrap();
            assert!(l[..5].iter().all(|&c| c == 0), "{linkage:?}: {l:?}");
            assert!(l[5..].iter().all(|&c| c == 1), "{linkage:?}: {l:?}");
        }
    }

    #[test]
    fn cutting_at_n_leaves_every_row_alone_and_at_one_joins_them_all() {
        let x = blobs_with_outlier();
        let all = Agglomerative { n_clusters: x.rows(), linkage: Linkage::Ward }.fit(&x).unwrap();
        assert_eq!(all, (0..x.rows()).collect::<Vec<_>>());
        let one = Agglomerative { n_clusters: 1, linkage: Linkage::Average }.fit(&x).unwrap();
        assert!(one.iter().all(|&c| c == 0));
    }

    #[test]
    fn single_linkage_chains_where_complete_linkage_does_not() {
        // A long evenly spaced line plus a tight pair far above its middle.
        // Single linkage keeps the line whole (every gap is 1); complete linkage
        // refuses a cluster that wide.
        let mut rows: Vec<Vec<f64>> = (0..10).map(|i| vec![i as f64, 0.0]).collect();
        rows.push(vec![4.5, 5.0]);
        rows.push(vec![4.6, 5.0]);
        let x = Matrix::from_rows(&rows).unwrap();
        let single = Agglomerative { n_clusters: 2, linkage: Linkage::Single }.fit(&x).unwrap();
        assert!(single[..10].iter().all(|&c| c == single[0]), "{single:?}");
        assert_ne!(single[10], single[0]);
        let complete = Agglomerative { n_clusters: 2, linkage: Linkage::Complete }.fit(&x).unwrap();
        assert!(complete[..10].iter().any(|&c| c != complete[0]), "{complete:?}");
    }

    #[test]
    fn indices_prefer_the_true_partition_over_a_scrambled_one() {
        let x = blobs_with_outlier().select_rows(&(0..10).collect::<Vec<_>>());
        let good: Vec<usize> = (0..10).map(|i| usize::from(i >= 5)).collect();
        let bad: Vec<usize> = (0..10).map(|i| i % 2).collect();
        let (sg, sb) = (silhouette(&x, &good).unwrap(), silhouette(&x, &bad).unwrap());
        assert!(sg > 0.9 && sg > sb, "silhouette {sg} vs {sb}");
        assert!(davies_bouldin(&x, &good).unwrap() < davies_bouldin(&x, &bad).unwrap());
        assert!(calinski_harabasz(&x, &good).unwrap() > calinski_harabasz(&x, &bad).unwrap());
    }

    #[test]
    fn an_index_refuses_a_single_cluster() {
        let x = blobs_with_outlier();
        assert!(silhouette(&x, &vec![0; x.rows()]).is_err());
    }
}
