use std::io::Cursor;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=image.png");
    let ico_path = Path::new("assets").join("app.ico");
    if let Err(err) = write_icon("image.png", &ico_path) {
        panic!("could not build icon from image.png: {err}");
    }

    #[cfg(windows)]
    {
        let mut resource = winres::WindowsResource::new();
        resource.set_icon(ico_path.to_str().expect("icon path"));
        resource
            .compile()
            .expect("could not embed windows exe icon");
    }
}

fn write_icon(png_path: &str, ico_path: &Path) -> Result<(), String> {
    let png = std::fs::read(png_path).map_err(|err| err.to_string())?;
    let image = image::load_from_memory(&png)
        .map_err(|err| err.to_string())?
        .into_rgba8();
    std::fs::create_dir_all(
        ico_path
            .parent()
            .ok_or_else(|| "missing icon folder".to_string())?,
    )
    .map_err(|err| err.to_string())?;

    let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
    for size in [16_u32, 32, 48, 64, 128, 256] {
        let resized = image::imageops::resize(
            &image,
            size,
            size,
            image::imageops::FilterType::Lanczos3,
        );
        let entry = ico::IconImage::from_rgba_data(size, size, resized.into_raw());
        icon_dir
            .add_entry(ico::IconDirEntry::encode(&entry).map_err(|err| err.to_string())?);
    }
    let mut bytes = Vec::new();
    icon_dir
        .write(&mut Cursor::new(&mut bytes))
        .map_err(|err| err.to_string())?;
    std::fs::write(ico_path, bytes).map_err(|err| err.to_string())
}
