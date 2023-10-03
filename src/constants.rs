use std::time::Duration;

use backon::ConstantBuilder;
use indicatif::{MultiProgress, ProgressStyle};
use lazy_static::lazy_static;

lazy_static! {
    pub static ref MULTIPROGRESS: MultiProgress = MultiProgress::new();
    pub static ref SPINNER_STYLE: ProgressStyle = ProgressStyle::default_spinner()
        .template("{spinner:.blue} {msg}")
        .unwrap();
    pub static ref SYNC_BAR_STYLE: ProgressStyle = ProgressStyle::default_bar()
        .template("{msg} {bar:40.cyan/blue} {pos:>7}/{len:7} {eta}")
        .unwrap();
    pub static ref CONSTANT_RETRY: ConstantBuilder = ConstantBuilder::default()
        .with_max_times(6)
        .with_delay(Duration::from_millis(200));
}
