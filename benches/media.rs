use std::hint::black_box;

use bzz::media::{decode::decode_image, imeta::parse_tags};
use criterion::{Criterion, criterion_group, criterion_main};
use image::ImageFormat;
use sha2::{Digest as _, Sha256};
use url::Url;

fn bench_media(c: &mut Criterion) {
    let temporary = tempfile::TempDir::new().unwrap();
    let path = temporary.path().join("fixture.png");
    let image = image::DynamicImage::new_rgb8(1024, 768);
    image.save_with_format(&path, ImageFormat::Png).unwrap();
    c.bench_function("decode bounded 1024x768 png", |bench| {
        bench.iter(|| black_box(decode_image(black_box(&path)).unwrap()))
    });

    let bytes = std::fs::read(&path).unwrap();
    let hash = hex::encode(Sha256::digest(&bytes));
    let url = format!("https://buzz.example/media/{hash}.png");
    let tags = serde_json::to_string(&vec![vec![
        "imeta".to_owned(),
        format!("url {url}"),
        "m image/png".into(),
        format!("x {hash}"),
        format!("size {}", bytes.len()),
        "dim 1024x768".into(),
    ]])
    .unwrap();
    let base = Url::parse("https://buzz.example/").unwrap();
    c.bench_function("parse bounded imeta", |bench| {
        bench.iter(|| black_box(parse_tags(black_box(&tags), "", black_box(&base))))
    });
}

criterion_group!(benches, bench_media);
criterion_main!(benches);
