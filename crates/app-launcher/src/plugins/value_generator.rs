//! Value generator plugin (`#`), after upstream ValueGenerator:
//! `uuid`/`guid` (v4, `uuidv7`), `md5`/`sha1`/`sha256`/`sha384`/`sha512 <text>`,
//! `base64 <text>`, `base64d <b64>`, `url <text>`, `urld <text>`.

use super::{LauncherResult, QueryCtx};
use crate::actions::Action;
use crate::config::PluginId;
use base64::Engine as _;
use sha1::Digest;

/// Outcome of parsing a value-generator request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    pub value: String,
    pub description: String,
}

const COMMANDS: &[(&str, &str)] = &[
    (
        "uuid",
        "Generate a random UUID (v4); uuidv7 for time-ordered",
    ),
    ("guid", "Generate a random GUID (v4)"),
    ("md5", "MD5 hash of text"),
    ("sha1", "SHA-1 hash of text"),
    ("sha256", "SHA-256 hash of text"),
    ("sha384", "SHA-384 hash of text"),
    ("sha512", "SHA-512 hash of text"),
    ("base64", "Base64-encode text"),
    ("base64d", "Base64-decode text"),
    ("url", "URL-encode text"),
    ("urld", "URL-decode text"),
];

pub fn generate(query: &str) -> Result<Generated, String> {
    let query = query.trim();
    let (cmd, rest) = match query.split_once(char::is_whitespace) {
        Some((c, r)) => (c, r.trim()),
        None => (query, ""),
    };
    let cmd_l = cmd.to_lowercase();
    let need_text = |what: &str| -> Result<(), String> {
        if rest.is_empty() {
            Err(format!("Type the text to {what}"))
        } else {
            Ok(())
        }
    };
    let ok = |value: String, description: String| Ok(Generated { value, description });
    match cmd_l.as_str() {
        "uuid" | "guid" | "uuidv4" | "guidv4" | "uuid4" | "guid4" => {
            ok(uuid::Uuid::new_v4().to_string(), "UUID v4".into())
        }
        "uuidv7" | "guidv7" | "uuid7" | "guid7" => {
            ok(uuid::Uuid::now_v7().to_string(), "UUID v7".into())
        }
        "md5" => {
            need_text("hash")?;
            ok(hex(&md5(rest.as_bytes())), format!("MD5 of \"{rest}\""))
        }
        "sha1" => {
            need_text("hash")?;
            ok(
                hex(&sha1::Sha1::digest(rest)),
                format!("SHA-1 of \"{rest}\""),
            )
        }
        "sha256" => {
            need_text("hash")?;
            ok(
                hex(&sha2::Sha256::digest(rest)),
                format!("SHA-256 of \"{rest}\""),
            )
        }
        "sha384" => {
            need_text("hash")?;
            ok(
                hex(&sha2::Sha384::digest(rest)),
                format!("SHA-384 of \"{rest}\""),
            )
        }
        "sha512" => {
            need_text("hash")?;
            ok(
                hex(&sha2::Sha512::digest(rest)),
                format!("SHA-512 of \"{rest}\""),
            )
        }
        "base64" => {
            need_text("encode")?;
            ok(
                base64::engine::general_purpose::STANDARD.encode(rest),
                "Base64 encoded".into(),
            )
        }
        "base64d" => {
            need_text("decode")?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(rest)
                .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(rest))
                .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(rest))
                .map_err(|e| format!("Invalid Base64: {e}"))?;
            let text =
                String::from_utf8(bytes).map_err(|_| "Decoded data is not UTF-8".to_string())?;
            ok(text, "Base64 decoded".into())
        }
        "url" => {
            need_text("encode")?;
            ok(url_encode(rest), "URL encoded".into())
        }
        "urld" => {
            need_text("decode")?;
            ok(url_decode(rest)?, "URL decoded".into())
        }
        _ => Err(format!("Unknown command \"{cmd}\"")),
    }
}

pub fn search(ctx: &QueryCtx) -> Vec<LauncherResult> {
    let q = ctx.search.trim();
    let first = q.split_whitespace().next().unwrap_or("").to_lowercase();
    match generate(q) {
        Ok(g) => vec![
            LauncherResult::new(
                PluginId::ValueGenerator,
                g.value.clone(),
                Action::Copy(g.value),
            )
            .subtitle(format!("{} · Enter to copy", g.description))
            .score(300.0),
        ],
        Err(err) => {
            // Show matching commands as hints while typing the command name.
            let hints: Vec<LauncherResult> = COMMANDS
                .iter()
                .filter(|(c, _)| first.is_empty() || c.starts_with(&first))
                .map(|(c, d)| {
                    let kw = ctx.config.plugin(PluginId::ValueGenerator).action_keyword;
                    LauncherResult::new(
                        PluginId::ValueGenerator,
                        *c,
                        Action::SetQuery(format!("{kw}{c} ")),
                    )
                    .subtitle(*d)
                    .score(100.0)
                })
                .collect();
            if hints.is_empty() || q.contains(char::is_whitespace) {
                vec![LauncherResult::new(PluginId::ValueGenerator, err, Action::None).score(100.0)]
            } else {
                hints
            }
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Percent-encode everything except RFC 3986 unreserved characters.
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Decode `%XX` escapes and `+` (as space).
pub fn url_decode(s: &str) -> Result<String, String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let h = bytes
                    .get(i + 1..i + 3)
                    .and_then(|h| std::str::from_utf8(h).ok())
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
                    .ok_or_else(|| format!("Invalid escape at position {i}"))?;
                out.push(h);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| "Decoded data is not UTF-8".into())
}

/// MD5 (RFC 1321). Implemented locally to avoid an extra dependency.
pub fn md5(input: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: Vec<u32> = (0..64)
        .map(|i| ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32)
        .collect();
    let (mut a0, mut b0, mut c0, mut d0) =
        (0x67452301u32, 0xefcdab89u32, 0x98badcfeu32, 0x10325476u32);

    let mut msg = input.to_vec();
    let bit_len = (input.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in msg.chunks(64) {
        let m: Vec<u32> = chunk
            .chunks(4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
            .collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | (!b & d), i),
                16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = [0u8; 16];
    out[..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..].copy_from_slice(&d0.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn val(q: &str) -> String {
        generate(q).unwrap().value
    }

    #[test]
    fn md5_vectors() {
        assert_eq!(hex(&md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(val("md5 abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            hex(&md5(b"The quick brown fox jumps over the lazy dog")),
            "9e107d9d372bb6826bd81d3542a419d6"
        );
        // Multi-block input (> 55 bytes)
        assert_eq!(hex(&md5(&[b'a'; 100])), "36a92cc94a9e0fa21f625f8bfb007adf");
    }

    #[test]
    fn sha_vectors() {
        assert_eq!(val("sha1 abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            val("SHA256 abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(val("sha512 abc").starts_with("ddaf35a193617aba"));
    }

    #[test]
    fn base64_roundtrip() {
        assert_eq!(val("base64 hello world"), "aGVsbG8gd29ybGQ=");
        assert_eq!(val("base64d aGVsbG8gd29ybGQ="), "hello world");
        assert_eq!(val("base64d aGVsbG8gd29ybGQ"), "hello world");
        assert!(generate("base64d ***").is_err());
    }

    #[test]
    fn url_roundtrip() {
        assert_eq!(val("url a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
        assert_eq!(val("urld a%20b%26c+d%C3%A9"), "a b&c dé");
        assert!(generate("urld %zz").is_err());
    }

    #[test]
    fn uuids() {
        let u = val("uuid");
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "4");
        assert_eq!(&val("guidv7")[14..15], "7");
        assert_ne!(val("guid"), val("guid"));
    }

    #[test]
    fn errors() {
        assert!(generate("md5").is_err());
        assert!(generate("foo bar").is_err());
    }
}
