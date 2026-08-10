#[cfg(all(test, feature = "usb-msc"))]
mod e2e_test;

#[cfg(all(test, feature = "usb-msc"))]
pub mod fat16;
