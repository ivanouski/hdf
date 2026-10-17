use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{env, fs};

// A file in the temp directory, removed when dropped, also when the test fails
pub struct TempFile {
    pub path: PathBuf,
}

impl TempFile {
    // The name is for finding a file left behind, a number keeps the paths of tests apart
    pub fn new(name: &str, content: &str) -> Self {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let number = COUNT.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!("hdf-test-{name}-{}-{number}", std::process::id()));
        fs::write(&path, content).unwrap();

        Self { path }
    }

    pub fn read(&self) -> String {
        fs::read_to_string(&self.path).unwrap()
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
