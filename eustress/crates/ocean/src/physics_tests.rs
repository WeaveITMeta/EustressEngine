//! Physics acceptance tests: each checks the synthesised sea against an
//! analytic or independently computed value, so a wrong sign, factor of
//! two, unit or band edge fails loudly rather than looking like water.

use std::f64::consts::TAU;

use crate::cascade::{frequency, Cascade};
use crate::fft::{Complex, Fft2d};
use crate::ocean::{Detail, Ocean, OceanSettings};
use crate::sea_state::pm_significant_wave_height;
use crate::spectrum::{Dispersion, Spectrum, EARTH_GRAVITY};

const G: f32 = EARTH_GRAVITY;

/// A cascade of side `n` over a `tile` metre tile that owns every mode.
fn open_cascade(n: usize, tile: f32) -> Cascade {
    let mut c = Cascade::new(0, n);
    c.configure(tile, 0.0, f32::INFINITY);
    c
}

/// Phase p of mode (fx, fz) in a field a cos(k.x - p) sampled over a
/// cascade's tile (component 1 of the displacement is the height): the
/// projection onto cos and sin of k.x.
fn mode_phase(c: &Cascade, component: usize, fx: i32, fz: i32) -> f64 {
    let n = c.resolution();
    let (mut cs, mut sn) = (0.0f64, 0.0f64);
    for iz in 0..n {
        for ix in 0..n {
            let m = (fx as i64 * ix as i64 + fz as i64 * iz as i64).rem_euclid(n as i64);
            let theta = TAU * m as f64 / n as f64;
            let v = c.displacement()[iz * n + ix][component] as f64;
            cs += v * theta.cos();
            sn += v * theta.sin();
        }
    }
    sn.atan2(cs)
}

/// Every displacement and derivative of two seas, bit for bit.
fn assert_identical(a: &Ocean, b: &Ocean) {
    let bits = |c: &Cascade| -> Vec<u32> {
        c.displacement().iter().chain(c.derivatives()).flat_map(|t| t.map(f32::to_bits)).collect()
    };
    for (ca, cb) in a.cascades().iter().zip(b.cascades()) {
        assert!(bits(ca) == bits(cb), "cascade {} differs", ca.index());
    }
}

/// Element i of the Halton sequence in `base`: well spread points in [0, 1).
fn halton(mut i: u32, base: u32) -> f32 {
    let (mut f, mut r) = (1.0f64, 0.0f64);
    while i > 0 {
        f /= base as f64;
        r += f * (i % base) as f64;
        i /= base;
    }
    r as f32
}

/// A lone wave travels toward +k at the deep-water phase speed sqrt(g / k),
/// in any direction on the lattice. The phase is read from the whole tile,
/// accurate to f32 round-off (about 1e-6), so 1e-4 is a wide margin; a
/// wrong time sign gives a negative speed and a wrong dispersion another
/// value.
#[test]
fn single_mode_travels_at_the_deep_water_phase_speed() {
    let n = 64;
    let fft = Fft2d::new(n);
    let deep = Dispersion::deep(G);
    for (fx, fz) in [(4, 0), (0, 5), (3, -2), (-6, 1)] {
        let mut c = open_cascade(n, 100.0);
        c.set_single_mode(fx, fz, Complex::new(0.25, 0.0), &deep);
        let [kx, kz] = c.wavevector(fx, fz);
        let k = (kx as f64).hypot(kz as f64);
        c.evaluate(0.0, 0.0, &fft);
        assert!(mode_phase(&c, 1, fx, fz).abs() < 1e-5, "mode ({fx}, {fz}): the crest starts at the origin");
        let peak = c.displacement().iter().map(|d| d[1]).fold(f32::MIN, f32::max);
        assert!((peak - 0.5).abs() < 1e-5, "mode ({fx}, {fz}): amplitude 2 |h0| = 0.5, got {peak}");
        // A third of a period on, the crest has moved a third of a wavelength.
        let t = TAU / 3.0 / (G as f64 * k).sqrt();
        c.evaluate(t, 0.0, &fft);
        let speed = mode_phase(&c, 1, fx, fz) / (k * t);
        let expect = (G as f64 / k).sqrt();
        assert!((speed / expect - 1.0).abs() < 1e-4, "mode ({fx}, {fz}): {speed} m/s vs {expect}");
    }
}

/// Over shallow water the same wave is slower: c = sqrt(g tanh(kh) / k).
#[test]
fn shallow_water_slows_a_wave_by_tanh_kh() {
    let n = 64;
    let fft = Fft2d::new(n);
    let (fx, fz) = (4, 0);
    for kh in [0.3f64, 1.0, 2.5] {
        let mut c = open_cascade(n, 100.0);
        let k = c.wavevector(fx, fz)[0] as f64;
        let depth = (kh / k) as f32;
        c.set_single_mode(fx, fz, Complex::new(0.25, 0.0), &Dispersion::new(G, depth));
        let kh = k * depth as f64;
        let expect = (G as f64 * kh.tanh() / k).sqrt();
        let t = TAU / 3.0 / (expect * k);
        c.evaluate(t, 0.0, &fft);
        let speed = mode_phase(&c, 1, fx, fz) / (k * t);
        assert!((speed / expect - 1.0).abs() < 1e-4, "kh {kh}: {speed} m/s vs {expect}");
        assert!(speed < (G as f64 / k).sqrt());
    }
}

/// Choppy displacement moves water toward the crests (Gerstner): for
/// eta = a cos(kx), Dx = -lambda a sin(kx), the slope is -a k sin(kx) and
/// J = 1 - lambda a k cos(kx), below 1 under crests and above 1 under
/// troughs. Texel by texel to 1e-5 (f32 round-off is about 1e-6 here).
#[test]
fn choppiness_sharpens_crests_and_flattens_troughs() {
    let n = 64;
    let fft = Fft2d::new(n);
    let fx = 4;
    let mut c = open_cascade(n, 100.0);
    let k = c.wavevector(fx, 0)[0];
    let a = 0.3 / k; // steepness a k = 0.3
    c.set_single_mode(fx, 0, Complex::new(0.5 * a, 0.0), &Dispersion::deep(G));
    for lambda in [1.0f32, 0.5, 0.0] {
        c.evaluate(0.0, lambda, &fft);
        for iz in [0usize, 17] {
            for ix in 0..n {
                let theta = (TAU * (fx as usize * ix) as f64 / n as f64) as f32;
                let d = c.displacement()[iz * n + ix];
                let v = c.derivatives()[iz * n + ix];
                let expect_d = [-lambda * a * theta.sin(), a * theta.cos(), 0.0, 0.0];
                let expect_v = [-a * k * theta.sin(), 0.0, 1.0 - lambda * a * k * theta.cos(), 0.0];
                for j in 0..4 {
                    assert!((d[j] - expect_d[j]).abs() < 1e-5, "lambda {lambda}, texel {ix}: displacement {d:?} vs {expect_d:?}");
                    assert!((v[j] - expect_v[j]).abs() < 1e-5, "lambda {lambda}, texel {ix}: derivatives {v:?} vs {expect_v:?}");
                }
            }
        }
        if lambda > 0.0 {
            // The highest texel is the most compressed one.
            let row = 0..n;
            let crest = row.clone().max_by(|&i, &j| c.displacement()[i][1].total_cmp(&c.displacement()[j][1])).unwrap();
            let least_j = row.map(|i| c.derivatives()[i][2]).fold(f32::MAX, f32::min);
            assert!((c.derivatives()[crest][2] - least_j).abs() < 1e-5 && least_j < 1.0);
        }
    }
}

/// The inverse transform of one field built from h(k, t) by its definition.
fn synthesise_alone(c: &Cascade, h: &[Complex], fft: &Fft2d, field: impl Fn(f32, f32, f32, Complex) -> Complex) -> Vec<Complex> {
    let n = c.resolution();
    let mut data: Vec<Complex> = (0..n * n)
        .map(|i| {
            let [kx, kz] = c.wavevector(frequency(i % n, n), frequency(i / n, n));
            let k = (kx * kx + kz * kz).sqrt();
            if k > 0.0 {
                field(kx, kz, k, h[i])
            } else {
                Complex::ZERO
            }
        })
        .collect();
    let mut scratch = vec![Complex::ZERO; n * n];
    fft.inverse(&mut data, &mut scratch);
    data
}

/// The four packed transforms unpack into the eight fields that eight
/// separate transforms give, each built from h(k, t) by its definition, and
/// each field alone synthesises to a real field. Tolerance 1e-5 of the
/// largest field value (f32 round-off is about 1e-6 of it at N = 32); a
/// swapped field, sign or factor of i errs by the field's own size.
#[test]
fn packed_fields_match_separate_transforms() {
    let n = 32;
    let fft = Fft2d::new(n);
    let settings = OceanSettings { wind_speed: 9.0, swell: 0.6, swell_direction: [0.3, 1.0], ..Default::default() }.sanitised();
    let spectrum = Spectrum::from_settings(&settings);
    let mut c = open_cascade(n, 150.0);
    c.build(&spectrum, 11);
    let (t, lambda) = (4.25, 0.8f32);
    c.evaluate(t, lambda, &fft);
    let h = c.spectrum_at(t);
    let i1 = Complex::new(0.0, 1.0);
    // Dx = i lambda (kx / k) h; a derivative d/dx multiplies by i kx.
    let height = synthesise_alone(&c, &h, &fft, |_, _, _, h| h);
    let dx = synthesise_alone(&c, &h, &fft, |kx, _, k, h| i1 * h.scale(lambda * kx / k));
    let dz = synthesise_alone(&c, &h, &fft, |_, kz, k, h| i1 * h.scale(lambda * kz / k));
    let slope_x = synthesise_alone(&c, &h, &fft, |kx, _, _, h| i1 * h.scale(kx));
    let slope_z = synthesise_alone(&c, &h, &fft, |_, kz, _, h| i1 * h.scale(kz));
    let dxx = synthesise_alone(&c, &h, &fft, |kx, _, k, h| i1 * (i1 * h.scale(lambda * kx / k)).scale(kx));
    let dzz = synthesise_alone(&c, &h, &fft, |_, kz, k, h| i1 * (i1 * h.scale(lambda * kz / k)).scale(kz));
    let dxz = synthesise_alone(&c, &h, &fft, |kx, kz, k, h| i1 * (i1 * h.scale(lambda * kx / k)).scale(kz));
    let fields = [&height, &dx, &dz, &slope_x, &slope_z, &dxx, &dzz, &dxz];
    let scale = fields.iter().flat_map(|f| f.iter().map(|v| v.re.abs())).fold(0.0f32, f32::max);
    assert!(scale > 0.1, "the test needs a busy sea, largest value {scale}");
    let tol = 1e-5 * scale.max(1.0);
    for i in 0..n * n {
        for f in fields {
            assert!(f[i].im.abs() < tol, "imaginary residue {} at {i}", f[i].im);
        }
        let d = c.displacement()[i];
        let v = c.derivatives()[i];
        let jacobian = (1.0 + dxx[i].re) * (1.0 + dzz[i].re) - dxz[i].re * dxz[i].re;
        let pairs = [
            (d[0], dx[i].re),
            (d[1], height[i].re),
            (d[2], dz[i].re),
            (d[3], 0.0),
            (v[0], slope_x[i].re),
            (v[1], slope_z[i].re),
            (v[2], jacobian),
            (v[3], 0.0),
        ];
        for (j, (packed, alone)) in pairs.into_iter().enumerate() {
            assert!((packed - alone).abs() < tol, "texel {i}, output {j}: packed {packed} vs alone {alone}");
        }
    }
}

/// The cascades carry the Pierson-Moskowitz height: Hs from their expected
/// variance is within 10% of 0.21 U^2 / g at 7, 11 and 16 m/s. The spectrum
/// itself gives 0.2093 U^2 / g and the bands hold all but about 3e-5 of
/// its variance, so the margin is for the lattice sum.
#[test]
fn expected_height_is_pierson_moskowitz() {
    for u in [7.0f32, 11.0, 16.0] {
        let ocean = Ocean::new(OceanSettings { wind_speed: u, swell: 0.0, ..Default::default() });
        let hs = ocean.significant_wave_height();
        let pm = pm_significant_wave_height(u, G);
        eprintln!("U {u} m/s: lattice Hs {hs:.4} m, spectrum {:.4} m, 0.21 U^2/g {pm:.4} m", ocean.spectrum().significant_wave_height());
        assert!((hs / pm - 1.0).abs() < 0.10, "U {u}: Hs {hs} m vs {pm} m");
    }
}

/// Relative standard error of one seed's realised height variance. The
/// pair (k, -k) contributes 2 |h(k)|^2, exponentially distributed about its
/// mean 2 sigma_k^2 with sigma_k^2 = (S(k) + S(-k)) dk^2 / 2, so the sum
/// over the lattice has mean sum_k sigma_k^2 and variance
/// 2 sum_k sigma_k^4 (sums over all lattice points).
fn realised_variance_relative_error(ocean: &Ocean) -> f64 {
    let (mut sum2, mut sum4) = (0.0f64, 0.0f64);
    for c in ocean.cascades() {
        let half = (c.resolution() / 2) as i32;
        let cell = (c.lattice_step() as f64).powi(2);
        for fz in -half..half {
            for fx in -half..half {
                if !c.owns(fx, fz) {
                    continue;
                }
                let [kx, kz] = c.wavevector(fx, fz);
                let s = ocean.spectrum().wavenumber_density(kx, kz) as f64 + ocean.spectrum().wavenumber_density(-kx, -kz) as f64;
                let sigma2 = s * cell / 2.0;
                sum2 += sigma2;
                sum4 += sigma2 * sigma2;
            }
        }
    }
    (2.0 * sum4).sqrt() / sum2
}

/// What a seed realises agrees with what the spectrum expects: the tiles'
/// height variance, averaged over four seeds, is within 15% of the expected
/// variance. The standard error of that average is computed from the
/// lattice, and the test checks that 15% is at least three of them, so the
/// bound is neither loose nor a coin toss.
#[test]
fn realised_variance_matches_the_expected_variance() {
    let seeds = [1u64, 2, 3, 4];
    let mut ratio = 0.0f64;
    let mut standard_error = 0.0f64;
    for seed in seeds {
        let mut ocean = Ocean::new(OceanSettings { swell: 0.0, seed, ..Default::default() });
        ocean.update(3.0);
        let realised: f64 = ocean
            .cascades()
            .iter()
            .map(|c| c.displacement().iter().map(|d| (d[1] as f64).powi(2)).sum::<f64>() / c.displacement().len() as f64)
            .sum();
        ratio += realised / ocean.expected_variance() / seeds.len() as f64;
        standard_error = realised_variance_relative_error(&ocean) / (seeds.len() as f64).sqrt();
    }
    eprintln!("realised / expected variance {ratio:.4}, standard error {standard_error:.4}");
    assert!(3.0 * standard_error < 0.15, "four seeds are too few: standard error {standard_error}");
    assert!((ratio - 1.0).abs() < 0.15, "realised / expected variance {ratio}");
}

/// Under half the gravity the same wind raises a sea twice as high and
/// twice as long: Hs = 0.21 U^2 / g and lambda_p = 2 pi U^2 / (0.877^2 g).
#[test]
fn halving_gravity_doubles_the_height() {
    let earth = Ocean::new(OceanSettings { swell: 0.0, ..Default::default() });
    let half = Ocean::new(OceanSettings { swell: 0.0, gravity: G / 2.0, ..Default::default() });
    let ratio = half.significant_wave_height() / earth.significant_wave_height();
    assert!((ratio / 2.0 - 1.0).abs() < 0.10, "Hs ratio {ratio}");
    let length = half.peak_wavelength() / earth.peak_wavelength();
    assert!((length / 2.0 - 1.0).abs() < 1e-4, "peak wavelength ratio {length}");
}

/// The bands meet edge to edge, and together the lattices carry the
/// spectrum's variance over the covered band within 5%: no mode is counted
/// twice and no band is left out; what remains is the lattice sum's
/// quadrature error. The covered band itself holds over 99% of the
/// spectrum.
#[test]
fn cascades_cover_the_spectrum_once() {
    let cases = [
        ("default", OceanSettings::default()),
        ("wind sea", OceanSettings { swell: 0.0, ..Default::default() }),
        ("low detail", OceanSettings { swell: 0.0, detail: Detail::Low, ..Default::default() }),
        ("high detail", OceanSettings { swell: 0.0, detail: Detail::High, ..Default::default() }),
        ("fetch", OceanSettings { wind_speed: 11.0, fetch: Some(40_000.0), swell: 0.0, ..Default::default() }),
        ("swell", OceanSettings { wind_speed: 0.0, swell: 1.0, ..Default::default() }),
        ("shallow", OceanSettings { depth: 15.0, ..Default::default() }),
    ];
    for (name, settings) in cases {
        let ocean = Ocean::new(settings);
        let cascades = ocean.cascades();
        assert_eq!(cascades[0].band()[0], 0.0, "{name}");
        for pair in cascades.windows(2) {
            assert_eq!(pair[0].band()[1], pair[1].band()[0], "{name}: gap or overlap between bands");
        }
        for c in cascades {
            let [lo, hi] = c.band();
            eprintln!(
                "{name}, cascade {} ({:.1} m): lattice {:.4e} m^2, band {:.4e} m^2",
                c.index(),
                c.tile_size(),
                c.expected_variance(),
                ocean.spectrum().variance_in_band(lo, hi)
            );
        }
        let covered = ocean.spectrum().variance_in_band(0.0, cascades[2].band()[1]);
        let lattice = ocean.expected_variance();
        assert!((lattice / covered - 1.0).abs() < 0.05, "{name}: lattice {lattice:e} vs band {covered:e}");
        assert!(covered / ocean.spectrum().variance() > 0.99, "{name}: the bands miss part of the spectrum");
    }
}

/// Swell alone at full setting is 2 m significant height. The band
/// coverage test bounds the variance to 5%, so the height to 2.5%.
#[test]
fn full_swell_is_two_metres() {
    let ocean = Ocean::new(OceanSettings { wind_speed: 0.0, swell: 1.0, ..Default::default() });
    let hs = ocean.significant_wave_height();
    assert!((hs / 2.0 - 1.0).abs() < 0.025, "Hs {hs} m");
}

/// The sea travels toward the wave direction: over the lattice, the
/// spectrum-weighted mean direction of the wavevectors (each of which
/// travels toward +k, as the single-mode test shows) is the setting's. A
/// sign error would point it the other way.
#[test]
fn waves_travel_toward_the_wave_direction() {
    let angle = 40.0f32.to_radians();
    let ocean = Ocean::new(OceanSettings { wave_direction: [angle.cos(), angle.sin()], swell: 0.0, ..Default::default() });
    let (mut sx, mut sz) = (0.0f64, 0.0f64);
    for c in ocean.cascades() {
        let half = (c.resolution() / 2) as i32;
        let cell = (c.lattice_step() as f64).powi(2);
        for fz in -half..half {
            for fx in -half..half {
                if !c.owns(fx, fz) {
                    continue;
                }
                let [kx, kz] = c.wavevector(fx, fz);
                let s = ocean.spectrum().wavenumber_density(kx, kz) as f64 * cell;
                let k = (kx as f64).hypot(kz as f64);
                sx += s * kx as f64 / k;
                sz += s * kz as f64 / k;
            }
        }
    }
    let mean = sz.atan2(sx).to_degrees();
    assert!((mean - 40.0).abs() < 2.0, "mean direction {mean} degrees");
}

/// Two seas with the same settings and time are bit for bit the same; a
/// different seed gives a different sea.
#[test]
fn same_settings_give_bit_identical_seas() {
    let settings = OceanSettings { seed: 42, ..Default::default() };
    let (mut a, mut b) = (Ocean::new(settings), Ocean::new(settings));
    a.update(12.5);
    b.update(12.5);
    assert_identical(&a, &b);
    let mut c = Ocean::new(OceanSettings { seed: 43, ..settings });
    c.update(12.5);
    assert_ne!(a.height_at(10.0, 20.0), c.height_at(10.0, 20.0));
}

/// The same sea from a pool of one thread and of four threads, bit for bit.
#[test]
fn fields_do_not_depend_on_the_thread_count() {
    let run = |threads: usize| {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().expect("thread pool");
        pool.install(|| {
            let mut ocean = Ocean::new(OceanSettings { detail: Detail::High, seed: 9, ..Default::default() });
            ocean.update(7.75);
            ocean
        })
    };
    let (one, four) = (run(1), run(4));
    assert_identical(&one, &four);
    for (x, z) in [(0.0f32, 0.0f32), (123.4, -56.7), (-999.0, 3.25)] {
        assert_eq!(one.height_at(x, z).to_bits(), four.height_at(x, z).to_bits());
    }
}

/// Largest change between neighbouring texels of a cascade's displacement,
/// per metre: it bounds the slope of the bilinear surface in x and in z.
fn max_gradient(c: &Cascade) -> f32 {
    let n = c.resolution();
    let d = c.displacement();
    let mut step = 0.0f32;
    for iz in 0..n {
        for ix in 0..n {
            let here = d[iz * n + ix];
            for there in [d[iz * n + (ix + 1) % n], d[((iz + 1) % n) * n + ix]] {
                for j in 0..3 {
                    step = step.max((there[j] - here[j]).abs());
                }
            }
        }
    }
    step / c.texel_size()
}

/// Sampling wraps each tile with its period, so the surface has no seam
/// where a tile repeats: across every tile edge the displacement, of one
/// cascade or summed, changes no more than its steepest texel step allows
/// over the distance crossed (a thousandth of a texel). A seam would jump
/// by the size of the field.
#[test]
fn displacement_is_continuous_across_tile_edges() {
    let mut ocean = Ocean::new(OceanSettings::default());
    ocean.update(5.0);
    let gradients: Vec<f32> = ocean.cascades().iter().map(max_gradient).collect();
    let total: f32 = gradients.iter().sum();
    for (c, &gradient) in ocean.cascades().iter().zip(&gradients) {
        let tile = c.tile_size();
        let delta = 1e-3 * c.texel_size();
        for edge in [tile, -tile, 3.0 * tile] {
            for j in 0..16 {
                let along = -37.0 + 23.3 * j as f32;
                for (p, q) in [((edge - delta, along), (edge + delta, along)), ((along, edge - delta), (along, edge + delta))] {
                    let crossed = (q.0 - p.0).abs() + (q.1 - p.1).abs();
                    let (a, b) = (c.sample(p.0, p.1), c.sample(q.0, q.1));
                    let (sa, sb) = (ocean.displacement_at(p.0, p.1), ocean.displacement_at(q.0, q.1));
                    for i in 0..3 {
                        assert!(
                            (a[i] - b[i]).abs() <= 1.001 * gradient * crossed + 1e-6,
                            "cascade {} seam at {p:?}: {a:?} vs {b:?}",
                            c.index()
                        );
                        assert!((sa[i] - sb[i]).abs() <= 1.001 * total * crossed + 1e-5, "seam in the sum at {p:?}: {sa:?} vs {sb:?}");
                    }
                }
            }
        }
        // A whole tile apart, a cascade reads the same, up to the rounding
        // of the shifted coordinate.
        for j in 0..16 {
            let (x, z) = (-50.0 + 11.1 * j as f32, 7.7 * j as f32);
            let shifted = x + tile;
            let rounding = ((shifted as f64 - tile as f64) - x as f64).abs() as f32;
            let (a, b) = (c.sample(x, z), c.sample(shifted, z));
            for i in 0..3 {
                assert!((a[i] - b[i]).abs() <= 1.001 * gradient * rounding + 1e-6, "cascade {} not periodic at {x}", c.index());
            }
        }
    }
}

/// height_at inverts the horizontal displacement: the surface point it
/// finds lies within 1 cm horizontally of the query, at 400 points of the
/// default sea (choppiness 1).
#[test]
fn height_inversion_lands_on_the_query_point() {
    let mut ocean = Ocean::new(OceanSettings::default());
    ocean.update(21.0);
    let (mut worst, mut largest_shift) = (0.0f32, 0.0f32);
    for i in 1..=400u32 {
        let (x, z) = (-400.0 + 800.0 * halton(i, 2), -400.0 + 800.0 * halton(i, 3));
        let p = ocean.surface_point(x, z);
        worst = worst.max((p[0] - x).hypot(p[2] - z));
        let d = ocean.displacement_at(x, z);
        largest_shift = largest_shift.max(d[0].hypot(d[2]));
        assert_eq!(p[1].to_bits(), ocean.height_at(x, z).to_bits());
    }
    eprintln!("worst horizontal miss {worst:e} m; largest horizontal displacement {largest_shift} m");
    assert!(largest_shift > 0.1, "the test needs real displacement, largest {largest_shift} m");
    assert!(worst < 0.01, "worst miss {worst} m");
}

/// No wind and no swell is still water: every displacement and slope is
/// exactly 0 and the Jacobian exactly 1 (undisturbed water is not
/// compressed), however many updates run. So is a world without gravity.
#[test]
fn calm_sea_is_exactly_flat() {
    let mut ocean = Ocean::new(OceanSettings { wind_speed: 0.0, swell: 0.0, ..Default::default() });
    assert!(ocean.is_flat());
    ocean.update(0.0);
    ocean.update(2.5);
    for c in ocean.cascades() {
        assert!(c.displacement().iter().all(|d| *d == [0.0; 4]));
        assert!(c.derivatives().iter().all(|d| *d == [0.0, 0.0, 1.0, 0.0]));
        assert_eq!(c.expected_variance(), 0.0);
    }
    assert_eq!(ocean.displacement_at(12.0, -7.0), [0.0; 3]);
    assert_eq!(ocean.height_at(12.0, -7.0), 0.0);
    assert_eq!(ocean.surface_point(12.0, -7.0), [12.0, 0.0, -7.0]);
    assert_eq!(ocean.vertical_velocity_at(12.0, -7.0), 0.0);
    assert_eq!(ocean.significant_wave_height(), 0.0);
    assert!(Ocean::new(OceanSettings { gravity: 0.0, ..Default::default() }).is_flat());
    assert!(!Ocean::new(OceanSettings { wind_speed: 0.0, ..Default::default() }).is_flat());
    assert!(!Ocean::new(OceanSettings { swell: 0.0, ..Default::default() }).is_flat());
}

/// set_settings rebuilds only for fields that shape the spectrum: the same
/// settings, or a new choppiness, keep the amplitudes and every buffer; a
/// new wind draws them again into the same buffers and gives the sea a
/// fresh ocean with those settings has.
#[test]
fn set_settings_rebuilds_only_when_the_spectrum_changes() {
    let mut ocean = Ocean::new(OceanSettings::default());
    ocean.update(1.0);
    let builds = ocean.spectrum_builds();
    let buffer = ocean.cascades()[0].displacement().as_ptr();
    ocean.set_settings(OceanSettings::default());
    assert_eq!(ocean.spectrum_builds(), builds);
    assert_eq!(ocean.cascades()[0].displacement().as_ptr(), buffer);
    ocean.set_settings(OceanSettings { choppiness: 0.5, ..Default::default() });
    assert_eq!(ocean.spectrum_builds(), builds);
    let windier = OceanSettings { wind_speed: 9.0, choppiness: 0.5, ..Default::default() };
    ocean.set_settings(windier);
    assert_eq!(ocean.spectrum_builds(), builds + 1);
    assert_eq!(ocean.cascades()[0].displacement().as_ptr(), buffer, "a rebuild at the same detail reuses the buffers");
    ocean.update(4.0);
    let mut fresh = Ocean::new(windier);
    fresh.update(4.0);
    assert_identical(&ocean, &fresh);
}

/// The vertical velocity is the change of height_at between the last two
/// updates over their step: 0 until two updates have run, and again after a
/// settings change, then equal to the difference of two independent seas at
/// those times.
#[test]
fn vertical_velocity_is_the_rate_of_rise_between_updates() {
    let settings = OceanSettings { seed: 5, ..Default::default() };
    let mut ocean = Ocean::new(settings);
    assert_eq!(ocean.vertical_velocity_at(1.0, 2.0), 0.0);
    ocean.update(10.0);
    assert_eq!(ocean.vertical_velocity_at(1.0, 2.0), 0.0);
    ocean.update(10.05);
    let (mut before, mut after) = (Ocean::new(settings), Ocean::new(settings));
    before.update(10.0);
    after.update(10.05);
    let dt = 10.05f64 - 10.0;
    let mut moving = false;
    for (x, z) in [(0.0f32, 0.0f32), (31.5, -12.25), (-250.0, 400.0)] {
        let expect = ((after.height_at(x, z) as f64 - before.height_at(x, z) as f64) / dt) as f32;
        let got = ocean.vertical_velocity_at(x, z);
        assert_eq!(got.to_bits(), expect.to_bits(), "at ({x}, {z}): {got} vs {expect}");
        moving |= got.abs() > 1e-3;
    }
    assert!(moving, "the sea should be moving");
    ocean.set_settings(OceanSettings { choppiness: 0.7, ..settings });
    ocean.update(10.1);
    assert_eq!(ocean.vertical_velocity_at(31.5, -12.25), 0.0);
}
