#[cfg(all(test, feature = "memcached"))]
mod e2e_test;

#[cfg(all(test, feature = "memcached"))]
mod real_client_test;
