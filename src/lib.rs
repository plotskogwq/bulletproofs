#![cfg_attr(feature = "bench", feature(test))]
#![feature(nll)]

#![feature(test)]

extern crate curve25519_dalek;
extern crate sha2;
extern crate rand;
extern crate byteorder;
extern crate tiny_keccak;

#[cfg(test)]
extern crate test;

pub mod proof_transcript;
mod range_proof;
mod inner_product_proof;

pub mod scalar;

pub use range_proof::*;
