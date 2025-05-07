use std::sync::Once;

static LOG_INIT: Once = Once::new();

pub fn setup_logging() {
  LOG_INIT.call_once(|| {
    env_logger::builder()
      .is_test(true)
      .try_init()
      .unwrap_or_else(|e| eprintln!("Failed to init logger: {}", e));
    // Using try_init().unwrap_or_else(...) because init() will panic if called more than once.
    // is_test(true) often configures it to write to stdout for tests.
  });
}
