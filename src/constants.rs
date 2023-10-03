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
}
