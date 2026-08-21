//! AMQP client tests module

#[cfg(all(test, feature = "amqp"))]
mod command_channel_test;
#[cfg(all(test, feature = "amqp"))]
mod e2e_test;
