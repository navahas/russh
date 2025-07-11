// Copyright 2016 Pierre-Étienne Meunier
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//!
//! This module exports cipher names for use with [Preferred].
use std::borrow::Borrow;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::num::Wrapping;

use aes::{Aes128, Aes192, Aes256};
#[cfg(feature = "aws-lc-rs")]
use aws_lc_rs::aead::{AES_128_GCM as ALGORITHM_AES_128_GCM, AES_256_GCM as ALGORITHM_AES_256_GCM};
use byteorder::{BigEndian, ByteOrder};
use ctr::Ctr128BE;
use delegate::delegate;
use log::trace;
use once_cell::sync::Lazy;
#[cfg(all(not(feature = "aws-lc-rs"), feature = "ring"))]
use ring::aead::{AES_128_GCM as ALGORITHM_AES_128_GCM, AES_256_GCM as ALGORITHM_AES_256_GCM};
use ssh_encoding::Encode;
use tokio::io::{AsyncRead, AsyncReadExt};

use self::cbc::CbcWrapper;
use crate::mac::MacAlgorithm;
use crate::sshbuffer::SSHBuffer;
use crate::Error;

pub(crate) mod block;
pub(crate) mod cbc;
pub(crate) mod chacha20poly1305;
pub(crate) mod clear;
pub(crate) mod gcm;

use block::SshBlockCipher;
use chacha20poly1305::SshChacha20Poly1305Cipher;
use clear::Clear;
use gcm::GcmCipher;

pub(crate) trait Cipher {
    fn needs_mac(&self) -> bool {
        false
    }
    fn key_len(&self) -> usize;
    fn nonce_len(&self) -> usize {
        0
    }
    fn make_opening_key(
        &self,
        key: &[u8],
        nonce: &[u8],
        mac_key: &[u8],
        mac: &dyn MacAlgorithm,
    ) -> Box<dyn OpeningKey + Send>;
    fn make_sealing_key(
        &self,
        key: &[u8],
        nonce: &[u8],
        mac_key: &[u8],
        mac: &dyn MacAlgorithm,
    ) -> Box<dyn SealingKey + Send>;
}

/// `clear`
pub const CLEAR: Name = Name("clear");
/// `3des-cbc`
#[cfg(feature = "des")]
pub const TRIPLE_DES_CBC: Name = Name("3des-cbc");
/// `aes128-ctr`
pub const AES_128_CTR: Name = Name("aes128-ctr");
/// `aes192-ctr`
pub const AES_192_CTR: Name = Name("aes192-ctr");
/// `aes128-cbc`
pub const AES_128_CBC: Name = Name("aes128-cbc");
/// `aes192-cbc`
pub const AES_192_CBC: Name = Name("aes192-cbc");
/// `aes256-cbc`
pub const AES_256_CBC: Name = Name("aes256-cbc");
/// `aes256-ctr`
pub const AES_256_CTR: Name = Name("aes256-ctr");
/// `aes128-gcm@openssh.com`
pub const AES_128_GCM: Name = Name("aes128-gcm@openssh.com");
/// `aes256-gcm@openssh.com`
pub const AES_256_GCM: Name = Name("aes256-gcm@openssh.com");
/// `chacha20-poly1305@openssh.com`
pub const CHACHA20_POLY1305: Name = Name("chacha20-poly1305@openssh.com");
/// `none`
pub const NONE: Name = Name("none");

pub(crate) static _CLEAR: Clear = Clear {};
#[cfg(feature = "des")]
static _3DES_CBC: SshBlockCipher<CbcWrapper<des::TdesEde3>> = SshBlockCipher(PhantomData);
static _AES_128_CTR: SshBlockCipher<Ctr128BE<Aes128>> = SshBlockCipher(PhantomData);
static _AES_192_CTR: SshBlockCipher<Ctr128BE<Aes192>> = SshBlockCipher(PhantomData);
static _AES_256_CTR: SshBlockCipher<Ctr128BE<Aes256>> = SshBlockCipher(PhantomData);
static _AES_128_GCM: GcmCipher = GcmCipher(&ALGORITHM_AES_128_GCM);
static _AES_256_GCM: GcmCipher = GcmCipher(&ALGORITHM_AES_256_GCM);
static _AES_128_CBC: SshBlockCipher<CbcWrapper<Aes128>> = SshBlockCipher(PhantomData);
static _AES_192_CBC: SshBlockCipher<CbcWrapper<Aes192>> = SshBlockCipher(PhantomData);
static _AES_256_CBC: SshBlockCipher<CbcWrapper<Aes256>> = SshBlockCipher(PhantomData);
static _CHACHA20_POLY1305: SshChacha20Poly1305Cipher = SshChacha20Poly1305Cipher {};

pub static ALL_CIPHERS: &[&Name] = &[
    &CLEAR,
    &NONE,
    #[cfg(feature = "des")]
    &TRIPLE_DES_CBC,
    &AES_128_CTR,
    &AES_192_CTR,
    &AES_256_CTR,
    &AES_128_GCM,
    &AES_256_GCM,
    &AES_128_CBC,
    &AES_192_CBC,
    &AES_256_CBC,
    &CHACHA20_POLY1305,
];

pub(crate) static CIPHERS: Lazy<HashMap<&'static Name, &(dyn Cipher + Send + Sync)>> =
    Lazy::new(|| {
        let mut h: HashMap<&'static Name, &(dyn Cipher + Send + Sync)> = HashMap::new();
        h.insert(&CLEAR, &_CLEAR);
        h.insert(&NONE, &_CLEAR);
        #[cfg(feature = "des")]
        h.insert(&TRIPLE_DES_CBC, &_3DES_CBC);
        h.insert(&AES_128_CTR, &_AES_128_CTR);
        h.insert(&AES_192_CTR, &_AES_192_CTR);
        h.insert(&AES_256_CTR, &_AES_256_CTR);
        h.insert(&AES_128_GCM, &_AES_128_GCM);
        h.insert(&AES_256_GCM, &_AES_256_GCM);
        h.insert(&AES_128_CBC, &_AES_128_CBC);
        h.insert(&AES_192_CBC, &_AES_192_CBC);
        h.insert(&AES_256_CBC, &_AES_256_CBC);
        h.insert(&CHACHA20_POLY1305, &_CHACHA20_POLY1305);
        assert_eq!(h.len(), ALL_CIPHERS.len());
        h
    });

#[derive(Debug, PartialEq, Eq, Copy, Clone, Hash)]
pub struct Name(&'static str);
impl AsRef<str> for Name {
    fn as_ref(&self) -> &str {
        self.0
    }
}

impl Encode for Name {
    delegate! { to self.as_ref() {
        fn encoded_len(&self) -> Result<usize, ssh_encoding::Error>;
        fn encode(&self, writer: &mut impl ssh_encoding::Writer) -> Result<(), ssh_encoding::Error>;
    }}
}

impl Borrow<str> for &Name {
    fn borrow(&self) -> &str {
        self.0
    }
}

impl TryFrom<&str> for Name {
    type Error = ();
    fn try_from(s: &str) -> Result<Name, ()> {
        CIPHERS.keys().find(|x| x.0 == s).map(|x| **x).ok_or(())
    }
}

pub(crate) struct CipherPair {
    pub local_to_remote: Box<dyn SealingKey + Send>,
    pub remote_to_local: Box<dyn OpeningKey + Send>,
}

impl Debug for CipherPair {
    fn fmt(&self, _: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        Ok(())
    }
}

pub(crate) trait OpeningKey {
    fn packet_length_to_read_for_block_length(&self) -> usize {
        4
    }

    fn decrypt_packet_length(&self, seqn: u32, encrypted_packet_length: &[u8]) -> [u8; 4];

    fn tag_len(&self) -> usize;

    fn open<'a>(&mut self, seqn: u32, ciphertext_and_tag: &'a mut [u8]) -> Result<&'a [u8], Error>;
}

pub(crate) trait SealingKey {
    fn padding_length(&self, plaintext: &[u8]) -> usize;

    fn fill_padding(&self, padding_out: &mut [u8]);

    fn tag_len(&self) -> usize;

    fn seal(&mut self, seqn: u32, plaintext_in_ciphertext_out: &mut [u8], tag_out: &mut [u8]);

    fn write(&mut self, payload: &[u8], buffer: &mut SSHBuffer) {
        // https://tools.ietf.org/html/rfc4253#section-6
        //
        // The variables `payload`, `packet_length` and `padding_length` refer
        // to the protocol fields of the same names.
        trace!("writing, seqn = {:?}", buffer.seqn.0);

        let padding_length = self.padding_length(payload);
        trace!("padding length {:?}", padding_length);
        let packet_length = PADDING_LENGTH_LEN + payload.len() + padding_length;
        trace!("packet_length {:?}", packet_length);
        let offset = buffer.buffer.len();

        // Maximum packet length:
        // https://tools.ietf.org/html/rfc4253#section-6.1
        assert!(packet_length <= u32::MAX as usize);
        #[allow(clippy::unwrap_used)] // length checked
        (packet_length as u32).encode(&mut buffer.buffer).unwrap();

        assert!(padding_length <= u8::MAX as usize);
        buffer.buffer.push(padding_length as u8);
        buffer.buffer.extend(payload);
        self.fill_padding(buffer.buffer.resize_mut(padding_length));
        buffer.buffer.resize_mut(self.tag_len());

        #[allow(clippy::indexing_slicing)] // length checked
        let (plaintext, tag) =
            buffer.buffer[offset..].split_at_mut(PACKET_LENGTH_LEN + packet_length);

        self.seal(buffer.seqn.0, plaintext, tag);

        buffer.bytes += payload.len();
        // Sequence numbers are on 32 bits and wrap.
        // https://tools.ietf.org/html/rfc4253#section-6.4
        buffer.seqn += Wrapping(1);
    }
}

pub(crate) async fn read<R: AsyncRead + Unpin>(
    stream: &mut R,
    buffer: &mut SSHBuffer,
    cipher: &mut (dyn OpeningKey + Send),
) -> Result<usize, Error> {
    if buffer.len == 0 {
        let mut len = vec![0; cipher.packet_length_to_read_for_block_length()];

        stream.read_exact(&mut len).await?;
        trace!("reading, len = {:?}", len);
        {
            let seqn = buffer.seqn.0;
            buffer.buffer.clear();
            buffer.buffer.extend(&len);
            trace!("reading, seqn = {:?}", seqn);
            let len = cipher.decrypt_packet_length(seqn, &len);
            let len = BigEndian::read_u32(&len) as usize;

            if len > MAXIMUM_PACKET_LEN {
                return Err(Error::PacketSize(len));
            }

            buffer.len = len + cipher.tag_len();
            trace!("reading, clear len = {:?}", buffer.len);
        }
    }

    buffer.buffer.resize(buffer.len + 4);
    trace!("read_exact {:?}", buffer.len + 4);

    let l = cipher.packet_length_to_read_for_block_length();

    #[allow(clippy::indexing_slicing)] // length checked
    stream.read_exact(&mut buffer.buffer[l..]).await?;

    trace!("read_exact done");
    let seqn = buffer.seqn.0;
    let plaintext = cipher.open(seqn, &mut buffer.buffer)?;

    let padding_length = *plaintext.first().to_owned().unwrap_or(&0) as usize;
    trace!("reading, padding_length {:?}", padding_length);
    let plaintext_end = plaintext
        .len()
        .checked_sub(padding_length)
        .ok_or(Error::IndexOutOfBounds)?;

    // Sequence numbers are on 32 bits and wrap.
    // https://tools.ietf.org/html/rfc4253#section-6.4
    buffer.seqn += Wrapping(1);
    buffer.len = 0;

    // Remove the padding
    buffer.buffer.resize(plaintext_end + 4);

    Ok(plaintext_end + 4)
}

pub(crate) const PACKET_LENGTH_LEN: usize = 4;

const MINIMUM_PACKET_LEN: usize = 16;
const MAXIMUM_PACKET_LEN: usize = 256 * 1024;

const PADDING_LENGTH_LEN: usize = 1;

#[cfg(feature = "_bench")]
pub mod benchmark;
