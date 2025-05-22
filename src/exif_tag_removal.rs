// use std::{env, fs};
// use std::path::Path;
// use rexiv2::Metadata;

// fn is_jpeg(path: &Path) -> bool {
//     if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
//         let ext = ext.to_lowercase();
//         ext == "jpg" || ext == "jpeg"
//     } else {
//         false
//     }
// }

// fn remove_datetime_original(path: &Path) {
//     if let Some(path_str) = path.to_str() {
//         let mut meta = Metadata::new_from_path(path_str).unwrap();

//         meta.clear_tag("Exif.Photo.DateTimeOriginal");

//         if let Ok(software) = meta.get_tag_string("Exif.Image.Software") {
//             if software.contains("SH148") {
//                 meta.clear_tag("Exif.Image.Software");
//             }
//         }

//         meta.save_to_file(path_str).unwrap();
//         println!("Processed: {}", path_str);
//     }
// }

// fn main() {
//     let args: Vec<String> = env::args().collect();

//     if args.len() != 3 || args[1] != "-r" {
//         eprintln!("Usage: {} -r <image.jpg|*>", args[0]);
//         return;
//     }

//     let files: Vec<_> = if args[2] == "*" {
//         fs::read_dir(".")
//             .unwrap()
//             .filter_map(Result::ok)
//             .map(|e| e.path())
//             .filter(|p| is_jpeg(p))
//             .collect()
//     } else {
//         vec![Path::new(&args[2]).to_path_buf()]
//     };

//     for path in files {
//         remove_datetime_original(&path);
//     }
// }
