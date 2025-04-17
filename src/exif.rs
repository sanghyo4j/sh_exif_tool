mod file_func {
    use std::env;
    use std::fs;
    use std::io::{ self, Write };
    use std::path::PathBuf;

    pub fn get_directory_path() -> Result<PathBuf, io::Error> {
        let mut input_path = String::new();
        println!("Enter the directory path (leave blank for current directory): ");

        io::stdin().read_line(&mut input_path)?;
        let input_path = input_path.trim();

        if input_path.is_empty() {
            return env::current_dir();
        }

        let path = PathBuf::from(input_path);
        if path.exists() {
            Ok(path)
        } else {
            Err(io::Error::new(io::ErrorKind::NotFound, "유효하지 않은 경로입니다."))
        }
    }

    pub fn list_files_in_directory(path: &PathBuf) -> Result<Vec<PathBuf>, io::Error> {
        // let mut file_list = Vec::new();
        // for entry in fs::read_dir(path)? {
        //     let entry = entry?;
        //     let file_path = entry.path();
        //     if fs::metadata(&file_path)?.is_file() {
        //         file_list.push(file_path);
        //     }
        // }
        // Ok(file_list);

        let extensions = ["jpg", "jpeg", "png", "gif"];
        let mut image_files = Vec::new();
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if extensions.contains(&ext.to_str().unwrap().to_lowercase().as_str()) {
                        image_files.push(path);
                    }
                }
            }
        }
        Ok(image_files)
    }

    pub fn get_file_name(pathbuf: &PathBuf) -> Option<String> {
        pathbuf
            .file_name()
            .and_then(|os_str| os_str.to_str())
            .map(|str| str.to_owned())
    }

    pub fn get_file_size(pathbuf: &PathBuf) -> io::Result<u64> {
        let metadata = fs::metadata(pathbuf)?;
        Ok(metadata.len())
    }

    // pub fn rename_file(old_path: &str, new_path: &str) -> Result<(), std::io::Error> {
    //     // 파일명 변경 함수
    //     unimplemented!()
    // }

    pub fn save_as_jpg_with_new_name(input_path: &PathBuf) -> PathBuf {
        let mut output_path = input_path.to_path_buf();
        if let Some(stem) = output_path.file_stem() {
            let mut new_file_name = stem.to_owned();
            new_file_name.push("_NEW");
            if let Some(ext) = input_path.extension() {
                new_file_name.push(".");
                new_file_name.push(ext);
            }
            output_path.set_file_name(new_file_name);
        }
        let img = image::open(input_path).expect("이미지를 열 수 없습니다");
        img.save(&output_path).expect("이미지를 저장할 수 없습니다");
        output_path
    }
}

mod print_func {
    use std::io::{ self, ErrorKind };

    pub fn print_files_info_oneline(
        str_o_filename: Option<String>,
        str_o_filesize: u64,
        str_o_filedate: Result<String, io::Error>
    ) {
        if str_o_filename.is_none() {
            println!("No filename found");
            return;
        }

        let str_filename = str_o_filename.unwrap();
        let str_date = match str_o_filedate {
            Ok(date_str) => date_str,
            Err(e) => { String::from("N/A") }
        };

        println!("File Name: {}, Size: {} bytes, Date: {}", str_filename, str_o_filesize, str_date);
    }
}

mod exif_func {
    use std::fs::File;
    use std::io::{ self, Read, Error, ErrorKind };
    use std::io::BufReader;
    use std::path::PathBuf;
    use rexiv2::{ Metadata, Rexiv2Error };

    // rexif 및 exif는 너무 허술하고 자료가 없음
    // rexiv2은 자료도 많고, 도전해볼만함.
    // C++ 라이브러리의 바인딩 형태이므로, 도전해보자

    // rexif = "0.7.4"
    // use rexif::parse_buffer;
    // pub fn get_exif_date_by_using_rexif_only(path: &PathBuf ) -> Result<(), Box<dyn std::error::Error>> {
    //     // match parse_file(file_path.to_str().unwrap()) {
    //     //     Ok(exif) => {
    //     //         // println!("{} EXIF entries: {}", file_path.display(), exif.entries.len());
    //     //         // for entry in &exif.entries {
    //     //         //     println!("Tag: {}, Value: {}", entry.tag, entry.value);
    //     //         // }
    //     //         Ok(());
    //     //     }
    //     //     Err(e) => {
    //     //         println!("파일을 파싱하는 데 문제가 발생했습니다. 이미지를 다시 저장합니다.");
    //     //         let output_path = super::file_func::save_as_jpg_with_new_name(file_path);
    //     //         println!("이미지를 다시 저장했습니다. 다시 Exif 데이터를 파싱합니다.");
    //     //         get_exif_date(&output_path);
    //     //     }
    //     // }

    pub fn get_exif_date(image_path: &PathBuf) -> Result<String, io::Error> {
        match Metadata::new_from_path(&image_path) {
            Ok(metadata) =>
                match metadata.get_tag_string("Exif.Image.DateTime") {
                    Ok(date_str) => Ok(date_str),
                    Err(_) => Err(io::Error::new(ErrorKind::InvalidData, "EXIF date not found")),
                }
            Err(_) => Err(io::Error::new(ErrorKind::NotFound, "Failed to load EXIF data")),
        }
    }

    // pub fn add_new_exif_structure(input_path: &PathBuf) -> Result<(), io::Error> {
    //     let output_path = {
    //         let mut new_path = input_path.clone();
    //         let file_name = input_path.file_stem().ok_or("Invalid file name")?;
    //         let extension = input_path.extension().unwrap_or_default();
    //         new_path.set_file_name(
    //             format!("{}_NEW.{}", file_name.to_string_lossy(), extension.to_string_lossy())
    //         );
    //         new_path
    //     };

    //     let mut image_data = Vec::new();
    //     File::open(input_path)?.read_to_end(&mut image_data)?;

    //     let new_exif = create_basic_exif()?;

    //     let mut output_file = OpenOptions::new()
    //         .write(true)
    //         .create(true)
    //         .truncate(true)
    //         .open(&output_path)?;
    //     output_file.write_all(&new_exif)?;
    //     output_file.write_all(&image_data)?;

    //     println!("EXIF 태그를 추가한 이미지를 저장했습니다: {:?}", output_path);
    //     Ok(())
    // }

    pub fn create_basic_exif_structure() -> Result<Vec<u8>, io::Error> {
        let mut exif_data = Vec::new();

        // EXIF 기본 헤더 (APP1 마커)
        exif_data.extend_from_slice(
            &[
                0xff,
                0xe1, // APP1 마커
                0x00,
                0x2a, // 데이터 길이 (추후 수정)
                b'E',
                b'x',
                b'i',
                b'f', // EXIF 식별자
                0x00,
                0x00, // NULL 패딩
            ]
        );

        // 기본 태그 예시: Software
        let field = Field {
            tag: Tag::Software,
            ifd_num: In::PRIMARY,
            value: Value::Ascii(vec![b"Rust EXIF Creator".to_vec()]),
        };

        // EXIF 태그를 추가
        exif_data.extend_from_slice(&field.to_bytes()?);

        // 데이터 길이 수정
        let data_length = (exif_data.len() - 2) as u16; // APP1 헤더 제외
        exif_data[2] = (data_length >> 8) as u8;
        exif_data[3] = (data_length & 0xff) as u8;

        Ok(exif_data)
    }

    //     pub fn write_exif_tags(file_path: &str, exif_data: &ExifData) -> Result<(), std::io::Error> {
    //         // EXIF 태그 쓰기 함수
    //         unimplemented!()
    //     }

    //     pub fn parse_date_from_filename(file_name: &str) -> Option<String> {
    //         // 파일명에서 날짜 파싱 함수
    //         unimplemented!()
    //     }
}

fn main() -> Result<(), std::io::Error> {
    let dir_path = match file_func::get_directory_path() {
        Ok(dir_path) => {
            println!("your path: {:?}", dir_path);
            dir_path
        }
        Err(e) => {
            eprintln!("error: {}", e);
            return Err(e);
        }
    };

    let list_files_in_dir = match file_func::list_files_in_directory(&dir_path) {
        Ok(list_files_in_dir) => list_files_in_dir,
        Err(e) => {
            eprintln!("error: {}", e);
            return Err(e);
        }
    };

    // println!("your files:");
    // for file in list_files_in_dir {
    //     // match file_func::get_file_name(&file) { Some(ref value) => println!("{}", value), None => println!("None"), }
    //     match exif_func::get_exif_date(&file) {
    //         Ok(_) => println!("{}: EXIF 데이터를 성공적으로 읽었습니다.", file.to_str().unwrap()),
    //         Err(e) => eprintln!("error: {}", e),
    //     }
    // }

    println!("your files:");
    for file in list_files_in_dir {
        print_func::print_files_info_oneline(
            file_func::get_file_name(&file),
            file_func::get_file_size(&file)?,
            exif_func::get_exif_date(&file)
        );
    }

    // if let Some(mut exif_data) = exif_utils::read_exif_tags(&file)? {
    //     if exif_data.date.is_none() {
    //         if let Some(date) = exif_utils::parse_date_from_filename(&file) {
    //             exif_data.date = Some(date);
    //             exif_utils::write_exif_tags(&file, &exif_data)?;
    //             let new_file_name = format!("{}_new.jpg", file);
    //             file_func::rename_file(&file, &new_file_name)?;
    //         }
    //     }
    // }
    // }

    Ok(())
}
