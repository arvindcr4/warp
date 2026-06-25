use std::borrow::Cow;
use std::rc::Rc;

use rust_embed::RustEmbed;

use super::*;
use crate::r#async::executor::{Background, Foreground};
use crate::AssetProvider;

#[derive(Clone, Copy, RustEmbed)]
#[folder = "test_data"]
pub struct Assets;

// Implement the AssetProvider trait here (required by App::new).
impl AssetProvider for Assets {
    fn get(&self, path: &str) -> Result<Cow<'_, [u8]>> {
        match path {
            "animated.webp" => Ok(Cow::Borrowed(include_bytes!("../test_data/animated.webp"))),
            "cache-test.svg" => Ok(Cow::Borrowed(
                br##"<svg width="20" height="10" viewBox="0 0 20 10" xmlns="http://www.w3.org/2000/svg"><rect width="20" height="10" fill="#ffffff"/></svg>"##,
            )),
            "fit-test.rgba" => Ok(Cow::Borrowed(
                b"warp-img:rgba:4:2:\xff\x00\x00\xff\xff\x00\x00\xff\x00\x00\xff\xff\x00\x00\xff\xff\x00\xff\x00\xff\x00\xff\x00\xff\xff\xff\xff\xff\xff\xff\xff\xff",
            )),
            "numbers-1000ms.gif" => Ok(Cow::Borrowed(include_bytes!(
                "../../warpui/examples/assets/numbers-1000ms.gif"
            ))),
            _ => <Assets as RustEmbed>::get(path)
                .map(|f| f.data)
                .ok_or_else(|| anyhow!("no asset exists at path {}", path)),
        }
    }
}

fn new_asset_cache() -> AssetCache {
    AssetCache::new(
        Box::new(Assets),
        Foreground::test().into(),
        Background::default().into(),
    )
}

fn load_bundled_image(
    image_cache: &ImageCache,
    asset_cache: &AssetCache,
    path: &'static str,
    bounds: Vector2I,
    fit_type: FitType,
    animated_image_behavior: AnimatedImageBehavior,
) -> Rc<Image> {
    let image = image_cache.image(
        AssetSource::Bundled { path },
        bounds,
        fit_type,
        animated_image_behavior,
        CacheOption::BySize,
        None,
        asset_cache,
    );
    let AssetState::Loaded { data: image } = image else {
        panic!("Bundled asset should be available immediately!");
    };
    image
}

#[test]
fn test_passes_through_asset_cache_original() {
    let asset_cache = new_asset_cache();
    let image_cache = ImageCache::new();

    let source = AssetSource::Bundled { path: "local.png" };
    let image_asset: AssetState<ImageType> = asset_cache.load_asset(source.clone());
    let AssetState::Loaded { data: image } = image_asset else {
        panic!("Bundled asset should be available immediately!");
    };
    let ImageType::StaticBitmap { image } = image.as_ref() else {
        panic!("Expected static image but got dynamic image!");
    };
    let image_asset_weak = Arc::downgrade(image);

    let bounds = Vector2I::new(1024, 1024);
    let image = image_cache.image(
        source,
        bounds,
        FitType::Cover,
        AnimatedImageBehavior::FullAnimation,
        CacheOption::Original,
        None,
        &asset_cache,
    );
    let AssetState::Loaded { data: image } = image else {
        panic!("Bundled asset should be available immediately!");
    };
    let Image::Static(image) = image.as_ref() else {
        panic!("Expected static image but got dynamic image!");
    };

    // Assert that the image returned from the image cache and the asset stored
    // in the asset cache point to the same underlying data (i.e.: there were
    // no copies made).
    assert!(image_asset_weak.ptr_eq(&Arc::downgrade(image)));
}

#[test]
fn test_passes_through_asset_cache_original_when_target_size_matches_source_size() {
    let asset_cache = new_asset_cache();
    let image_cache = ImageCache::new();

    let source = AssetSource::Bundled { path: "local.png" };
    let image_asset: AssetState<ImageType> = asset_cache.load_asset(source.clone());
    let AssetState::Loaded { data: image } = image_asset else {
        panic!("Bundled asset should be available immediately!");
    };
    let ImageType::StaticBitmap { image } = image.as_ref() else {
        panic!("Expected static image but got dynamic image!");
    };
    let image_asset_weak = Arc::downgrade(image);

    // Load the image with `CacheOption::BySize` but use the source asset's
    // size as the bounds.
    let bounds = image.size();
    let image = image_cache.image(
        source,
        bounds,
        FitType::Cover,
        AnimatedImageBehavior::FullAnimation,
        CacheOption::BySize,
        None,
        &asset_cache,
    );
    let AssetState::Loaded { data: image } = image else {
        panic!("Bundled asset should be available immediately!");
    };
    let Image::Static(image) = image.as_ref() else {
        panic!("Expected static image but got dynamic image!");
    };

    // Assert that the image returned from the image cache and the asset stored
    // in the asset cache point to the same underlying data (i.e.: there were
    // no copies made).
    assert!(image_asset_weak.ptr_eq(&Arc::downgrade(image)));
}

#[test]
fn test_caches_svg_rendered_at_intrinsic_size() {
    let asset_cache = new_asset_cache();
    let image_cache = ImageCache::new();
    let bounds = Vector2I::new(20, 10);

    let image = load_bundled_image(
        &image_cache,
        &asset_cache,
        "cache-test.svg",
        bounds,
        FitType::Contain,
        AnimatedImageBehavior::FullAnimation,
    );
    let image_again = load_bundled_image(
        &image_cache,
        &asset_cache,
        "cache-test.svg",
        bounds,
        FitType::Contain,
        AnimatedImageBehavior::FullAnimation,
    );

    assert!(Rc::ptr_eq(&image, &image_again));
}

#[test]
fn test_different_fit_types_do_not_collide_in_rendered_image_cache() {
    let asset_cache = new_asset_cache();
    let image_cache = ImageCache::new();
    let bounds = Vector2I::new(8, 8);

    let cover = load_bundled_image(
        &image_cache,
        &asset_cache,
        "fit-test.rgba",
        bounds,
        FitType::Cover,
        AnimatedImageBehavior::FullAnimation,
    );
    let stretch = load_bundled_image(
        &image_cache,
        &asset_cache,
        "fit-test.rgba",
        bounds,
        FitType::Stretch,
        AnimatedImageBehavior::FullAnimation,
    );
    assert!(!Rc::ptr_eq(&cover, &stretch));
    let Image::Static(cover) = cover.as_ref() else {
        panic!("Expected static image but got animated image!");
    };
    let Image::Static(stretch) = stretch.as_ref() else {
        panic!("Expected static image but got animated image!");
    };
    assert_eq!(cover.img.dimensions(), (8, 8));
    assert_eq!(stretch.img.dimensions(), (8, 8));
    assert_ne!(cover.rgba_bytes(), stretch.rgba_bytes());
}

#[test]
fn test_respects_max_dimensions_for_cacheoption_original() {
    let asset_cache = new_asset_cache();
    let image_cache = ImageCache::new();

    // We pass a very small value for bounds, which should get ignored due to
    // use of `CacheOption::Original`.
    let bounds = Vector2I::new(10, 10);

    let image = image_cache.image(
        AssetSource::Bundled { path: "local.png" },
        bounds,
        FitType::Cover,
        AnimatedImageBehavior::FullAnimation,
        CacheOption::Original,
        None,
        &asset_cache,
    );
    let AssetState::Loaded { data: image } = image else {
        panic!("Bundled asset should be available immediately!");
    };

    let Image::Static(image) = image.as_ref() else {
        panic!("Expected static image but got dynamic image!");
    };
    // Assert that the image, without resizing or a max dimension, matches our expectations.
    assert_eq!(image.img.dimensions(), (1024, 1024));

    let image = image_cache.image(
        AssetSource::Bundled { path: "local.png" },
        bounds,
        FitType::Cover,
        AnimatedImageBehavior::FullAnimation,
        CacheOption::Original,
        Some(512),
        &asset_cache,
    );
    let AssetState::Loaded { data: image } = image else {
        panic!("Bundled asset should be available immediately!");
    };

    let Image::Static(image) = image.as_ref() else {
        panic!("Expected static image but got dynamic image!");
    };
    // Assert that, when we specify a max dimension of 512, the image is resized accordingly.
    assert_eq!(image.img.dimensions(), (512, 512));
}

#[test]
fn test_first_frame_preview_returns_static_for_animated_gif() {
    let asset_cache = new_asset_cache();
    let image_cache = ImageCache::new();

    let image = load_bundled_image(
        &image_cache,
        &asset_cache,
        "numbers-1000ms.gif",
        Vector2I::new(16, 16),
        FitType::Contain,
        AnimatedImageBehavior::FirstFramePreview,
    );

    let Image::Static(image) = image.as_ref() else {
        panic!("Expected static image but got animated image!");
    };
    assert_eq!(image.img.dimensions(), (16, 16));
}

#[test]
fn test_first_frame_preview_keeps_full_animation_in_asset_cache() {
    let asset_cache = new_asset_cache();
    let image_cache = ImageCache::new();

    for path in ["numbers-1000ms.gif", "animated.webp"] {
        let image = load_bundled_image(
            &image_cache,
            &asset_cache,
            path,
            Vector2I::new(16, 16),
            FitType::Contain,
            AnimatedImageBehavior::FirstFramePreview,
        );

        assert!(matches!(image.as_ref(), Image::Static(_)));

        let asset: AssetState<ImageType> = asset_cache.load_asset(AssetSource::Bundled { path });
        let AssetState::Loaded { data } = asset else {
            panic!("Animated asset should be available immediately!");
        };
        assert!(matches!(data.as_ref(), ImageType::AnimatedBitmap { .. }));
    }
}

#[test]
fn test_first_frame_preview_returns_static_for_animated_webp() {
    let asset_cache = new_asset_cache();
    let image_cache = ImageCache::new();

    let image = load_bundled_image(
        &image_cache,
        &asset_cache,
        "animated.webp",
        Vector2I::new(16, 16),
        FitType::Contain,
        AnimatedImageBehavior::FirstFramePreview,
    );

    let Image::Static(image) = image.as_ref() else {
        panic!("Expected static image but got animated image!");
    };
    assert_eq!(image.img.dimensions(), (16, 16));
}

#[test]
fn test_full_animation_still_returns_animated_for_gif_and_webp() {
    let asset_cache = new_asset_cache();
    let image_cache = ImageCache::new();

    for path in ["numbers-1000ms.gif", "animated.webp"] {
        let image = load_bundled_image(
            &image_cache,
            &asset_cache,
            path,
            Vector2I::new(16, 16),
            FitType::Contain,
            AnimatedImageBehavior::FullAnimation,
        );

        let Image::Animated(image) = image.as_ref() else {
            panic!("Expected animated image but got static image!");
        };
        assert!(image.frames.len() > 1);
    }
}

#[test]
fn test_first_frame_preview_does_not_regress_static_formats() {
    let asset_cache = new_asset_cache();
    let image_cache = ImageCache::new();

    let image = load_bundled_image(
        &image_cache,
        &asset_cache,
        "local.png",
        Vector2I::new(16, 16),
        FitType::Contain,
        AnimatedImageBehavior::FirstFramePreview,
    );

    let Image::Static(image) = image.as_ref() else {
        panic!("Expected static image but got animated image!");
    };
    assert_eq!(image.img.dimensions(), (16, 16));
}

#[test]
fn test_preview_and_full_animation_requests_do_not_collide_in_rendered_image_cache() {
    let asset_cache = new_asset_cache();
    let image_cache = ImageCache::new();
    let bounds = Vector2I::new(16, 16);

    let preview = load_bundled_image(
        &image_cache,
        &asset_cache,
        "numbers-1000ms.gif",
        bounds,
        FitType::Contain,
        AnimatedImageBehavior::FirstFramePreview,
    );
    let full = load_bundled_image(
        &image_cache,
        &asset_cache,
        "numbers-1000ms.gif",
        bounds,
        FitType::Contain,
        AnimatedImageBehavior::FullAnimation,
    );
    let preview_again = load_bundled_image(
        &image_cache,
        &asset_cache,
        "numbers-1000ms.gif",
        bounds,
        FitType::Contain,
        AnimatedImageBehavior::FirstFramePreview,
    );
    let full_again = load_bundled_image(
        &image_cache,
        &asset_cache,
        "numbers-1000ms.gif",
        bounds,
        FitType::Contain,
        AnimatedImageBehavior::FullAnimation,
    );

    assert!(matches!(preview.as_ref(), Image::Static(_)));
    assert!(matches!(full.as_ref(), Image::Animated(_)));
    assert!(Rc::ptr_eq(&preview, &preview_again));
    assert!(Rc::ptr_eq(&full, &full_again));
    assert!(!Rc::ptr_eq(&preview, &full));
}

#[test]
fn test_svg_text_rasterizes_with_loaded_system_fonts() {
    let image_type = ImageType::try_from_bytes(
        br##"<svg width="160" height="40" viewBox="0 0 160 40" xmlns="http://www.w3.org/2000/svg">
  <text x="10" y="24" font-size="20" fill="#000000">Warp</text>
</svg>
"##,
    )
    .expect("SVG should parse");
    let ImageType::Svg { svg } = &image_type else {
        panic!("Expected SVG image type");
    };
    let font_family = svg
        .fontdb()
        .faces()
        .flat_map(|face| face.families.iter().map(|(family, _)| family.as_str()))
        .find(|family| {
            matches!(
                *family,
                "Arial"
                    | "Helvetica"
                    | "Inter"
                    | "DejaVu Sans"
                    | "Liberation Sans"
                    | "Noto Sans"
                    | "Cantarell"
                    | "Segoe UI"
            )
        })
        .or_else(|| {
            svg.fontdb()
                .faces()
                .find_map(|face| face.families.first().map(|(family, _)| family.as_str()))
        })
        .expect("System fonts should be loaded");

    let svg = format!(
        "<svg width=\"160\" height=\"40\" viewBox=\"0 0 160 40\" xmlns=\"http://www.w3.org/2000/svg\">\
  <text x=\"10\" y=\"24\" font-family=\"{font_family}\" font-size=\"20\" fill=\"#000000\">Warp</text>\
</svg>"
    );

    let image_type =
        ImageType::try_from_bytes(svg.as_bytes()).expect("SVG with installed font should parse");
    let image = image_type
        .to_image(
            Vector2I::new(160, 40),
            FitType::Contain,
            true,
            AnimatedImageBehavior::FullAnimation,
        )
        .expect("SVG should rasterize");
    let Image::Static(image) = image else {
        panic!("Expected static image");
    };

    assert!(image
        .rgba_bytes()
        .chunks_exact(4)
        .any(|pixel| pixel[3] != 0));
}

#[test]
fn test_svg_text_rasterizes_with_bundled_sans_serif_fallback() {
    let image_type = ImageType::try_from_bytes(
        br##"<svg width="160" height="40" viewBox="0 0 160 40" xmlns="http://www.w3.org/2000/svg">
  <text x="10" y="24" font-family="sans-serif" font-size="20" fill="#000000">Warp</text>
</svg>
"##,
    )
    .expect("SVG should parse");
    let ImageType::Svg { svg } = &image_type else {
        panic!("Expected SVG image type");
    };

    assert!(svg.fontdb().faces().any(|face| {
        face.families
            .iter()
            .any(|(family, _)| family.as_str() == "Roboto")
    }));

    let image = image_type
        .to_image(
            Vector2I::new(160, 40),
            FitType::Contain,
            true,
            AnimatedImageBehavior::FullAnimation,
        )
        .expect("SVG should rasterize");
    let Image::Static(image) = image else {
        panic!("Expected static image");
    };

    assert!(image
        .rgba_bytes()
        .chunks_exact(4)
        .any(|pixel| pixel[3] != 0));
}

#[test]
fn test_evict_image_drops_arc_for_resized_bysize() {
    let asset_cache = new_asset_cache();
    let image_cache = ImageCache::new();
    let source = AssetSource::Bundled { path: "local.png" };

    // Request the image at a smaller size than its 1024x1024 source, which forces a resize
    // and allocates a fresh Arc<StaticImage> not shared with AssetCache.
    let bounds = Vector2I::new(64, 64);
    let weak = {
        let image = image_cache.image(
            source.clone(),
            bounds,
            FitType::Cover,
            AnimatedImageBehavior::FullAnimation,
            CacheOption::BySize,
            None,
            &asset_cache,
        );
        let AssetState::Loaded { data: image } = image else {
            panic!("Bundled asset should be available immediately!");
        };
        let Image::Static(arc) = image.as_ref() else {
            panic!("Expected static image!");
        };
        Arc::downgrade(arc)
        // The local Rc<Image> clone is dropped here; only ImageCache holds the entry now.
    };

    assert_eq!(
        weak.strong_count(),
        1,
        "ImageCache should be the sole strong holder after the caller drops its Rc clone"
    );

    // Evicting from ImageCache should make the Arc releasable by TextureCache.
    image_cache.evict_image(&source);
    assert_eq!(
        weak.strong_count(),
        0,
        "After evict_image, the resized Arc should have no strong holders (cascade invariant)"
    );
}

#[test]
fn test_evict_size_drops_arc_only_for_targeted_entry() {
    let asset_cache = new_asset_cache();
    let image_cache = ImageCache::new();
    let source = AssetSource::Bundled { path: "local.png" };

    // Cache the same asset at two distinct sizes.
    let small_bounds = Vector2I::new(32, 32);
    let large_bounds = Vector2I::new(256, 256);

    let weak_small = {
        let image = image_cache.image(
            source.clone(),
            small_bounds,
            FitType::Cover,
            AnimatedImageBehavior::FullAnimation,
            CacheOption::BySize,
            None,
            &asset_cache,
        );
        let AssetState::Loaded { data: image } = image else {
            panic!("Bundled asset should be available immediately!");
        };
        let Image::Static(arc) = image.as_ref() else {
            panic!("Expected static image!");
        };
        Arc::downgrade(arc)
    };

    let weak_large = {
        let image = image_cache.image(
            source.clone(),
            large_bounds,
            FitType::Cover,
            AnimatedImageBehavior::FullAnimation,
            CacheOption::BySize,
            None,
            &asset_cache,
        );
        let AssetState::Loaded { data: image } = image else {
            panic!("Bundled asset should be available immediately!");
        };
        let Image::Static(arc) = image.as_ref() else {
            panic!("Expected static image!");
        };
        Arc::downgrade(arc)
    };

    assert_eq!(weak_small.strong_count(), 1);
    assert_eq!(weak_large.strong_count(), 1);

    // Evict only the small size entry.
    image_cache.evict_size(
        &source,
        small_bounds,
        FitType::Cover,
        AnimatedImageBehavior::FullAnimation,
    );

    assert_eq!(
        weak_small.strong_count(),
        0,
        "Small size Arc should have no strong holders after evict_size"
    );
    assert_eq!(
        weak_large.strong_count(),
        1,
        "Large size Arc should remain alive; only the small size was evicted"
    );
}

#[test]
fn test_svg_image_size_returns_intrinsic_dimensions() {
    let image_type = ImageType::try_from_bytes(
        br##"<svg width="160" height="40" viewBox="0 0 160 40" xmlns="http://www.w3.org/2000/svg"></svg>"##,
    )
    .expect("SVG should parse");

    assert_eq!(image_type.image_size(), Some(Vector2I::new(160, 40)));
}

#[test]
fn test_respects_max_dimensions_for_cacheoption_bysize() {
    let asset_cache = new_asset_cache();
    let image_cache = ImageCache::new();

    let bounds = Vector2I::new(768, 768);

    let image = image_cache.image(
        AssetSource::Bundled { path: "local.png" },
        bounds,
        FitType::Cover,
        AnimatedImageBehavior::FullAnimation,
        CacheOption::BySize,
        None,
        &asset_cache,
    );
    let AssetState::Loaded { data: image } = image else {
        panic!("Bundled asset should be available immediately!");
    };

    let Image::Static(image) = image.as_ref() else {
        panic!("Expected static image but got dynamic image!");
    };
    // Assert that the image gets resized to match the provided bounds.
    assert_eq!(image.img.dimensions(), (768, 768));

    let image = image_cache.image(
        AssetSource::Bundled { path: "local.png" },
        bounds,
        FitType::Cover,
        AnimatedImageBehavior::FullAnimation,
        CacheOption::BySize,
        Some(512),
        &asset_cache,
    );
    let AssetState::Loaded { data: image } = image else {
        panic!("Bundled asset should be available immediately!");
    };

    let Image::Static(image) = image.as_ref() else {
        panic!("Expected static image but got dynamic image!");
    };
    // Assert that, when we specify a max dimension of 512, the image is resized accordingly.
    assert_eq!(image.img.dimensions(), (512, 512));
}

#[test]
fn test_cached_images_insert_with_budget_evicts_oldest_fifo() {
    let mut cached_images = CachedImages::default();

    let key_1 = RenderedImageCacheKey {
        bounds: Vector2I::new(1, 1),
        fit_type: FitType::Cover,
        animated_image_behavior: AnimatedImageBehavior::FullAnimation,
    };
    let key_2 = RenderedImageCacheKey {
        bounds: Vector2I::new(2, 2),
        fit_type: FitType::Cover,
        animated_image_behavior: AnimatedImageBehavior::FullAnimation,
    };
    let key_3 = RenderedImageCacheKey {
        bounds: Vector2I::new(3, 3),
        fit_type: FitType::Cover,
        animated_image_behavior: AnimatedImageBehavior::FullAnimation,
    };

    let image_1 = Rc::new(Image::Static(test_utils::make_static_image(3, 3)));
    let image_2 = Rc::new(Image::Static(test_utils::make_static_image(4, 2)));
    let image_3 = Rc::new(Image::Static(test_utils::make_static_image(5, 2)));
    let max_bytes = 100;

    cached_images.insert_with_budget(1, key_1, image_1, max_bytes);
    cached_images.insert_with_budget(2, key_2, image_2, max_bytes);
    cached_images.insert_with_budget(3, key_3, image_3, max_bytes);

    assert_eq!(cached_images.order.len(), 2);
    let first_key = cached_images.order.front().unwrap();
    let second_key = cached_images.order.back().unwrap();
    assert_eq!(first_key.0, 2);
    assert_eq!(first_key.1, key_2);
    assert_eq!(second_key.0, 3);
    assert_eq!(second_key.1, key_3);
    assert!(cached_images.map.contains_key(&3));
    assert!(cached_images.map.contains_key(&2));
    assert!(!cached_images.map.contains_key(&1));
    assert!(cached_images.total_bytes <= max_bytes);
}

#[test]
fn test_cached_images_single_large_image_is_retained_even_when_over_budget() {
    let mut cached_images = CachedImages::default();

    let key = RenderedImageCacheKey {
        bounds: Vector2I::new(4, 4),
        fit_type: FitType::Contain,
        animated_image_behavior: AnimatedImageBehavior::FullAnimation,
    };
    let image = Rc::new(Image::Static(test_utils::make_static_image(10, 10)));

    let max_bytes = 10;
    cached_images.insert_with_budget(1, key, image, max_bytes);

    assert_eq!(cached_images.order.len(), 1);
    assert_eq!(cached_images.total_bytes, 400);
    assert_eq!(cached_images.total_bytes > max_bytes, true);
}

#[test]
fn test_cached_images_keeps_total_bytes_exact_during_evicts() {
    let mut cached_images = CachedImages::default();

    let max_bytes = 160;

    let key_1 = RenderedImageCacheKey {
        bounds: Vector2I::new(1, 1),
        fit_type: FitType::Contain,
        animated_image_behavior: AnimatedImageBehavior::FullAnimation,
    };
    let key_2 = RenderedImageCacheKey {
        bounds: Vector2I::new(2, 2),
        fit_type: FitType::Contain,
        animated_image_behavior: AnimatedImageBehavior::FullAnimation,
    };
    let key_3 = RenderedImageCacheKey {
        bounds: Vector2I::new(3, 3),
        fit_type: FitType::Contain,
        animated_image_behavior: AnimatedImageBehavior::FullAnimation,
    };
    let key_4 = RenderedImageCacheKey {
        bounds: Vector2I::new(4, 4),
        fit_type: FitType::Contain,
        animated_image_behavior: AnimatedImageBehavior::FullAnimation,
    };

    cached_images.insert_with_budget(
        1,
        key_1,
        Rc::new(Image::Static(test_utils::make_static_image(4, 4))),
        max_bytes,
    ); // 64
    assert_eq!(cached_images.total_bytes, 64);

    cached_images.insert_with_budget(
        2,
        key_2,
        Rc::new(Image::Static(test_utils::make_static_image(5, 2))),
        max_bytes,
    ); // 40
    assert_eq!(cached_images.total_bytes, 104);

    cached_images.insert_with_budget(
        3,
        key_3,
        Rc::new(Image::Static(test_utils::make_static_image(3, 4))),
        max_bytes,
    ); // 48
    assert_eq!(cached_images.total_bytes, 152);

    cached_images.insert_with_budget(
        4,
        key_4,
        Rc::new(Image::Static(test_utils::make_static_image(2, 2))),
        max_bytes,
    ); // 16 => 168 then evict oldest 64
    assert_eq!(cached_images.total_bytes, 104);
    assert_eq!(cached_images.order.len(), 3);
    assert_eq!(cached_images.order.front().unwrap().0, 2);
    assert_eq!(cached_images.order.back().unwrap().0, 4);
    let order_sum = cached_images
        .order
        .iter()
        .map(|(_, _, bytes)| *bytes)
        .sum::<usize>();
    assert_eq!(order_sum, cached_images.total_bytes);
}

#[test]
fn test_cached_images_remove_asset_and_remove_size_adjust_bytes_and_clear_inner_maps() {
    let mut cached_images = CachedImages::default();
    let max_bytes = 1024;

    let key_a = RenderedImageCacheKey {
        bounds: Vector2I::new(1, 1),
        fit_type: FitType::Cover,
        animated_image_behavior: AnimatedImageBehavior::FullAnimation,
    };
    let key_b = RenderedImageCacheKey {
        bounds: Vector2I::new(2, 2),
        fit_type: FitType::Cover,
        animated_image_behavior: AnimatedImageBehavior::FullAnimation,
    };
    let key_c = RenderedImageCacheKey {
        bounds: Vector2I::new(3, 3),
        fit_type: FitType::Cover,
        animated_image_behavior: AnimatedImageBehavior::FullAnimation,
    };

    let image_a = Rc::new(Image::Static(test_utils::make_static_image(4, 4)));
    let image_b = Rc::new(Image::Static(test_utils::make_static_image(2, 2)));
    let image_c = Rc::new(Image::Static(test_utils::make_static_image(1, 4)));

    cached_images.insert_with_budget(1, key_a, image_a, max_bytes); // 64
    cached_images.insert_with_budget(1, key_b, image_b, max_bytes); // 16
    cached_images.insert_with_budget(2, key_c, image_c, max_bytes); // 16
    assert_eq!(cached_images.total_bytes, 96);
    assert_eq!(
        cached_images.map.get(&1).expect("asset should exist").len(),
        2
    );

    cached_images.remove_size(1, &key_b);
    assert_eq!(cached_images.total_bytes, 80);
    assert!(cached_images.map.contains_key(&1));
    assert_eq!(
        cached_images.map.get(&1).expect("asset should exist").len(),
        1
    );

    cached_images.remove_size(1, &key_a);
    assert_eq!(cached_images.total_bytes, 16);
    assert!(!cached_images.map.contains_key(&1));
    assert_eq!(cached_images.order.len(), 1);

    cached_images.remove_asset(2);
    assert_eq!(cached_images.total_bytes, 0);
    assert!(!cached_images.map.contains_key(&2));
    assert_eq!(cached_images.order.len(), 0);
}

#[test]
fn test_image_cache_hit_returns_same_rendered_image() {
    let asset_cache = new_asset_cache();
    let image_cache = ImageCache::new();

    let image_1 = load_bundled_image(
        &image_cache,
        &asset_cache,
        "fit-test.rgba",
        Vector2I::new(8, 8),
        FitType::Contain,
        AnimatedImageBehavior::FullAnimation,
    );
    let image_2 = load_bundled_image(
        &image_cache,
        &asset_cache,
        "fit-test.rgba",
        Vector2I::new(8, 8),
        FitType::Contain,
        AnimatedImageBehavior::FullAnimation,
    );

    assert!(Rc::ptr_eq(&image_1, &image_2));
}
