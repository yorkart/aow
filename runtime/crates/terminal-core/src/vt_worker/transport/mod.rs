mod client;
mod protocol;
#[cfg(test)]
mod tests;

pub(in crate::vt_worker) use client::RpcClient;
