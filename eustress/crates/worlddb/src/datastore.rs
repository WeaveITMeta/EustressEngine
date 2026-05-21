//! Phase 8 — Roblox-parity DataStore API over [`crate::WorldDb`].
//!
//! `DataStoreService:GetDataStore("PlayerData")` →
//! [`DataStoreService::get_data_store`] →
//! [`DataStore`] with `get_async` / `set_async` / `update_async` /
//! `remove_async`; [`OrderedDataStore`] adds `set_sorted` +
//! `get_sorted_page`; [`DataStorePages`] is a forward cursor over a
//! sorted range. Values are opaque bytes — the engine's Rune/Luau
//! bridge serialises script values to JSON before calling, exactly
//! as Roblox serialises Luau tables.
//!
//! Durability, atomic `UpdateAsync` CAS, and ordered range scans all
//! come from the Fjall backing (`ds_*` trait methods). This is the
//! persistence engine; the script-language bindings are a thin shim
//! the engine adds on top (it owns the Rune/Luau runtime, this crate
//! must stay engine-free).

use std::sync::Arc;

use crate::backend::WorldDb;
use crate::error::Result;

/// Default scope when a script doesn't pass one (Roblox default).
pub const DEFAULT_SCOPE: &str = "global";

/// Roblox `DataStoreService`. Cheap to clone — just an `Arc` to the
/// open world DB.
#[derive(Clone)]
pub struct DataStoreService {
    db: Arc<dyn WorldDb>,
}

impl DataStoreService {
    /// Wrap an open `WorldDb`. The engine constructs one per Space and
    /// exposes it to scripts.
    pub fn new(db: Arc<dyn WorldDb>) -> Self {
        Self { db }
    }

    /// `GetDataStore(name [, scope])`.
    pub fn get_data_store(&self, name: &str, scope: Option<&str>) -> DataStore {
        DataStore {
            db: self.db.clone(),
            name: name.to_string(),
            scope: scope.unwrap_or(DEFAULT_SCOPE).to_string(),
        }
    }

    /// `GetOrderedDataStore(name [, scope])`.
    pub fn get_ordered_data_store(&self, name: &str, scope: Option<&str>) -> OrderedDataStore {
        OrderedDataStore {
            db: self.db.clone(),
            name: name.to_string(),
            scope: scope.unwrap_or(DEFAULT_SCOPE).to_string(),
        }
    }
}

/// A standard key→value store (`GlobalDataStore` semantics).
#[derive(Clone)]
pub struct DataStore {
    db: Arc<dyn WorldDb>,
    name: String,
    scope: String,
}

impl DataStore {
    /// `GetAsync(key)` — `Ok(None)` if unset.
    pub fn get_async(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.db.ds_get(&self.name, &self.scope, key)
    }

    /// `SetAsync(key, value)` — unconditional overwrite.
    pub fn set_async(&self, key: &str, value: &[u8]) -> Result<()> {
        self.db.ds_set(&self.name, &self.scope, key, value)
    }

    /// `RemoveAsync(key)` — returns the prior value, Roblox-style.
    pub fn remove_async(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.db.ds_remove(&self.name, &self.scope, key)
    }

    /// `UpdateAsync(key, transformFunction)` — atomic compare-and-swap.
    /// `transform` gets the current value (or `None`) and returns the
    /// new value, or `None` to abort the update (Roblox: returning
    /// `nil` cancels). Retries up to `max_retries` on contention.
    pub fn update_async(
        &self,
        key: &str,
        max_retries: u32,
        mut transform: impl FnMut(Option<Vec<u8>>) -> Option<Vec<u8>>,
    ) -> Result<Option<Vec<u8>>> {
        self.db.ds_update(
            &self.name,
            &self.scope,
            key,
            max_retries,
            &mut transform,
        )
    }
}

/// A leaderboard-style store whose values are ranked by an i64.
#[derive(Clone)]
pub struct OrderedDataStore {
    db: Arc<dyn WorldDb>,
    name: String,
    scope: String,
}

impl OrderedDataStore {
    /// `SetAsync(key, value)` for an ordered store — `value` is the
    /// i64 the leaderboard ranks on. The raw bytes stored alongside
    /// are the canonical i64 LE so `GetAsync` round-trips.
    pub fn set_async(&self, key: &str, value: i64) -> Result<()> {
        self.db.ds_set_sorted(
            &self.name,
            &self.scope,
            key,
            &value.to_le_bytes(),
            value,
        )
    }

    /// `GetAsync(key)` — the i64 rank value, or `None`.
    pub fn get_async(&self, key: &str) -> Result<Option<i64>> {
        Ok(self.db.ds_get(&self.name, &self.scope, key)?.map(|b| {
            // Plain entry is `[sort_be8][value_le8]`; value is the
            // canonical i64. Fall back to 0 on a malformed row.
            if b.len() >= 16 {
                let mut a = [0u8; 8];
                a.copy_from_slice(&b[8..16]);
                i64::from_le_bytes(a)
            } else {
                0
            }
        }))
    }

    /// `GetSortedAsync(ascending, pagesize [, minValue, maxValue])` —
    /// returns the first page plus a [`DataStorePages`] cursor for
    /// `AdvanceToNextPageAsync`.
    pub fn get_sorted(
        &self,
        ascending: bool,
        page_size: usize,
        min: Option<i64>,
        max: Option<i64>,
    ) -> Result<DataStorePages> {
        let rows =
            self.db
                .ds_range(&self.name, &self.scope, ascending, page_size, min, max, "")?;
        let last = rows.last().map(|(k, _, _)| k.clone()).unwrap_or_default();
        Ok(DataStorePages {
            db: self.db.clone(),
            name: self.name.clone(),
            scope: self.scope.clone(),
            ascending,
            page_size,
            min,
            max,
            current: rows
                .into_iter()
                .map(|(k, _v, s)| (k, s))
                .collect(),
            cursor: last,
            is_finished: false,
        })
    }
}

/// Roblox `DataStorePages` — forward-only cursor over a sorted range.
pub struct DataStorePages {
    db: Arc<dyn WorldDb>,
    name: String,
    scope: String,
    ascending: bool,
    page_size: usize,
    min: Option<i64>,
    max: Option<i64>,
    current: Vec<(String, i64)>,
    cursor: String,
    is_finished: bool,
}

impl DataStorePages {
    /// `GetCurrentPage()` — `(key, i64)` pairs for the current page.
    pub fn get_current_page(&self) -> &[(String, i64)] {
        &self.current
    }

    /// `IsFinished` — true once a page came back short / empty.
    pub fn is_finished(&self) -> bool {
        self.is_finished
    }

    /// `AdvanceToNextPageAsync()` — loads the next page in place.
    pub fn advance(&mut self) -> Result<()> {
        if self.is_finished {
            return Ok(());
        }
        let rows = self.db.ds_range(
            &self.name,
            &self.scope,
            self.ascending,
            self.page_size,
            self.min,
            self.max,
            &self.cursor,
        )?;
        if rows.len() < self.page_size {
            self.is_finished = true;
        }
        self.cursor = rows.last().map(|(k, _, _)| k.clone()).unwrap_or_default();
        self.current = rows.into_iter().map(|(k, _v, s)| (k, s)).collect();
        Ok(())
    }
}
