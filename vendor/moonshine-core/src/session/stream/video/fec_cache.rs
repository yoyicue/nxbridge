//! Process-local, bounded cache of immutable protocol matrices. Session keys,
//! counters, image bytes and other player state never enter this cache.
use fec_rs::{Error, ReedSolomon};
use std::{
	collections::VecDeque,
	sync::{Arc, Mutex, OnceLock},
};

const CAPACITY: usize = 128;
type Shape = (usize, usize);

#[derive(Default)]
struct Cache {
	entries: VecDeque<(Shape, Arc<ReedSolomon>)>,
}

impl Cache {
	fn get(&mut self, shape: Shape) -> Option<Arc<ReedSolomon>> {
		let index = self.entries.iter().position(|(key, _)| *key == shape)?;
		let entry = self.entries.remove(index)?;
		let value = entry.1.clone();
		self.entries.push_back(entry);
		Some(value)
	}

	fn insert(&mut self, shape: Shape, value: Arc<ReedSolomon>) -> Arc<ReedSolomon> {
		if let Some(existing) = self.get(shape) {
			return existing;
		}
		if self.entries.len() == CAPACITY {
			self.entries.pop_front();
		}
		self.entries.push_back((shape, value.clone()));
		value
	}
}

static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();

pub(super) fn get_encoder(data: usize, parity: usize) -> Result<Arc<ReedSolomon>, Error> {
	if data == 0 {
		return Err(Error::TooFewDataShards);
	}
	if parity == 0 {
		return Err(Error::TooFewParityShards);
	}
	if data.checked_add(parity).is_none_or(|total| total > 255) {
		return Err(Error::TooManyShards);
	}
	let cache = CACHE.get_or_init(|| Mutex::new(Cache::default()));
	if let Some(value) = cache.lock().unwrap_or_else(|p| p.into_inner()).get((data, parity)) {
		return Ok(value);
	}
	// Moonlight nanors' Cauchy coefficients. Construction stays outside the
	// cache lock; a concurrent insertion wins when the lock is reacquired.
	let mut rows = Vec::with_capacity(data * parity);
	for row in 0..parity {
		for col in 0..data {
			rows.push(fec_rs::galois::div(1, ((parity + col) ^ row) as u8));
		}
	}
	let value = Arc::new(ReedSolomon::new_with_parity_matrix(data, parity, &rows)?);
	Ok(cache
		.lock()
		.unwrap_or_else(|p| p.into_inner())
		.insert((data, parity), value))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn bounded_lru_reuses_read_only_encoders_and_retains_live_borrowers() {
		let mut cache = Cache::default();
		let value = Arc::new(ReedSolomon::new(1, 1).unwrap());
		for count in 1..=CAPACITY {
			cache.insert((count, 1), value.clone());
		}
		let held = cache.get((1, 1)).unwrap();
		cache.insert((CAPACITY + 1, 1), value.clone());
		assert_eq!(cache.entries.len(), CAPACITY);
		assert!(cache.get((2, 1)).is_none());
		assert!(Arc::ptr_eq(&held, &cache.get((1, 1)).unwrap()));
		assert!(Arc::ptr_eq(
			&held,
			&cache.insert((1, 1), Arc::new(ReedSolomon::new(1, 1).unwrap()))
		));
	}

	#[test]
	fn repeated_protocol_shape_reuses_matrix_and_invalid_shapes_are_rejected() {
		assert!(Arc::ptr_eq(&get_encoder(17, 4).unwrap(), &get_encoder(17, 4).unwrap()));
		assert!(get_encoder(0, 2).is_err());
		assert!(get_encoder(200, 56).is_err());
		assert!(get_encoder(usize::MAX, 1).is_err());
	}
}
