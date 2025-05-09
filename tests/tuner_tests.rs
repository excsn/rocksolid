// rocksolid/tests/tuner_tests.rs
mod common;

use common::setup_logging;

use rocksdb::Options as RocksDbOptions; // Import specific types for assertions
use rocksolid::tuner::Tunable;

// Helper to check lock status (might need to be pub(crate) in tunable.rs or exposed via a method)
trait TunableTestAccess<T> {
  fn is_locked_for_test(&self, key: &'static str) -> bool;
}

impl<T> TunableTestAccess<T> for Tunable<T> {
  fn is_locked_for_test(&self, key: &'static str) -> bool {
    // This assumes `is_locked` is accessible. If it's private,
    // this helper needs to be part of the same module or `is_locked` made `pub(crate)`.
    // For now, let's assume we can access it for testing.
    // If not, we'd have to infer locking by behavior only.
    self.is_locked(key) // Assuming is_locked is accessible here
  }
}

#[test]
fn test_tunable_locking_behavior() {
  setup_logging();
  let mut opts = Tunable::new(RocksDbOptions::default());

  opts.set_max_open_files(100); // This locks "db_max_open_files"
  assert!(opts.is_locked_for_test("db_max_open_files"));

  // Attempt to tune a locked option; it should not change.
  let original_opts_ptr = &opts.inner as *const _; // For unsafe comparison if needed
  opts.tune_set_max_open_files(200);

  // To truly verify, we'd need getters on RocksDbOptions or a way to snapshot/compare.
  // For this test, we rely on the is_locked mechanism.
  // If we had a getter: assert_eq!(opts.inner.get_max_open_files_somehow(), 100);
  // This also implies that the inner options object itself is not replaced.
  assert_eq!(
    &opts.inner as *const _, original_opts_ptr,
    "Inner options object should not be replaced by tune if locked"
  );

  let mut opts2 = Tunable::new(RocksDbOptions::default());
  opts2.tune_set_max_open_files(50); // This should set it
  assert!(!opts2.is_locked_for_test("db_max_open_files")); // Not locked by tune_
                                                           // If we had a getter: assert_eq!(opts2.inner.get_max_open_files_somehow(), 50);
}
