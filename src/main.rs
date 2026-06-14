use clap::Parser;
use local_ip_address::local_ip;
use log::{error, info, warn};
use pathdiff::diff_paths;
use std::{
    fmt::Write,
    fs,
    io::{self, Cursor},
    path::PathBuf,
    process,
};
use tiny_http::{Header, Response, Server};

#[cfg(feature = "qrcode")]
use {
    clap::ArgAction,
    qrcode::{QrCode, render::unicode},
};

#[cfg(feature = "upload")]
use {
    clap::ValueEnum,
    multipart::server::Multipart,
    std::{io::Read, path::Path},
};

#[derive(Parser)]
struct Args {
    #[arg(short, long, default_value_t = 3000)]
    port: u16,

    #[arg(short, long, default_value = ".")]
    directory: PathBuf,

    #[cfg(feature = "qrcode")]
    #[arg(short = 'q', long = "no-qrcode", action = ArgAction::SetFalse, default_value_t = true)]
    show_qrcode: bool,

    #[cfg(feature = "upload")]
    #[arg(short, long)]
    upload: Option<DuplicateBehaviour>,
}

impl Args {
    #[inline]
    fn upload_enabled(&self) -> bool {
        #[cfg(feature = "upload")]
        {
            self.upload.is_some()
        }

        #[cfg(not(feature = "upload"))]
        {
            false
        }
    }
}

#[cfg(feature = "upload")]
#[derive(Clone, Copy, ValueEnum)]
enum DuplicateBehaviour {
    #[value(alias = "replace")]
    KeepNew,
    KeepBoth,
    KeepOld,
}

fn main() {
    env_logger::init();
    let args = Args::parse();

    let server = match Server::http(("0.0.0.0", args.port)) {
        Ok(server) => server,
        Err(e) => {
            error!("Could not start server: {}", e);
            let mut source = e.source();
            while let Some(err) = source {
                error!("Caused by: {}", err);
                source = err.source();
            }
            process::exit(1)
        }
    };
    info!("Starting server");
    info!("Serving directory: {}", args.directory.display());

    if let Ok(ip) = local_ip() {
        let url = format!("http://{ip}:{}", args.port);
        println!("Server running at:");
        println!("{url}");

        #[cfg(feature = "qrcode")]
        if args.show_qrcode {
            print_qr(&url);
        }
    } else {
        warn!("Local ip could not be resolved");
    }

    for mut request in server.incoming_requests() {
        let method = request.method();
        info!(
            "{} - {} {}",
            request.remote_addr().ip(),
            method,
            request.url()
        );

        let rel_path = request.url().trim_start_matches('/');
        let path = args.directory.join(rel_path);

        #[cfg(feature = "upload")]
        if let Some(duplicate_behaviour) = args.upload
            && *method == tiny_http::Method::Post
        {
            let Ok(mut mult) = Multipart::from_request(&mut request) else {
                let resp = Response::from_string("Can't parse request body").with_status_code(500);
                let _ = request.respond(resp);
                continue;
            };
            if mult
                .foreach_entry(|mut field| {
                    if field.headers.name != "file".into() {
                        warn!("Unrecognized form field {}", field.headers.name);
                        return;
                    }
                    let Some(filename) = field.headers.filename else {
                        return;
                    };

                    let mut data = Vec::new();
                    if field.data.read_to_end(&mut data).is_err() {
                        error!("Can't read transferred file {filename}");
                        return;
                    };

                    let dest = destination_path(&args.directory, &filename, &duplicate_behaviour);

                    match duplicate_behaviour {
                        DuplicateBehaviour::KeepOld if dest.exists() => {}
                        _ => match fs::write(&dest, &data) {
                            Ok(_) => info!("Written file {} to disk", dest.display()),
                            Err(err) => {
                                error!("Can't write file {}: {err}", dest.display());
                            }
                        },
                    }
                })
                .is_err()
            {
                error!("An error occorured reading the multipart form data");
            };
        }

        let resp = response(&path, &args.directory, args.upload_enabled())
            .unwrap_or_else(|e| error_response(e, path));

        let _ = request.respond(resp);
    }
}

#[cfg(feature = "qrcode")]
fn print_qr(url: &str) {
    let code = QrCode::new(url).unwrap();

    let image = code.render::<unicode::Dense1x2>().quiet_zone(true).build();

    println!("{image}");
}

fn response(path: &PathBuf, base: &PathBuf, upload: bool) -> io::Result<Response<Cursor<Vec<u8>>>> {
    if path.is_file() {
        let mime_type = mime_guess::from_path(path).first_or_octet_stream();

        let bytes = fs::read(path)?;
        Ok(Response::from_data(bytes)
            .with_status_code(200)
            .with_header(
                Header::from_bytes(&b"Content-Type"[..], mime_type.to_string().as_bytes()).unwrap(),
            ))
    } else if path.is_dir() {
        let entries = fs::read_dir(path)?;
        let display_path =
            diff_paths(path, base).map_or_else(|| "?".to_string(), |p| p.display().to_string());
        let mut body = if cfg!(feature = "upload") && upload {
            format!(
                "
            <!DOCTYPE html>
            <html>
            <body>
            <h1>Directory listing for <code>/{}</code></h1>
            <form method='POST' enctype='multipart/form-data'>
            <input type='file' name='file'>
            <button type='submit'>Upload</button>
            </form>
            <hr>
            <ul>
            ",
                display_path
            )
        } else {
            format!(
                "
            <!DOCTYPE html>
            <html>
            <body>
            <h1>Directory listing for <code>/{}</code></h1>
            <hr>
            <ul>
            ",
                display_path
            )
        };
        for entry in entries {
            let entry = entry?;
            let mut href = entry.file_name().to_str().unwrap_or_default().to_string();

            if entry.file_type()?.is_dir() {
                href.push('/');
            }

            let _ = write!(body, "<li><a href='{href}'>{href}</a></li>");
        }
        let _ = body.write_str(
            "
            </ul>
            <hr>
            </body>
            </html>
            ",
        );

        Ok(Response::from_string(body)
            .with_status_code(200)
            .with_header(html_header()))
    } else {
        let body = format!(
            "<!DOCTYPE html>
            <html>
            <body>
            <h1>File or Directory not found</h1>
            <p>Path: <code>{}</code></p>
            </body>
            </html>",
            path.display()
        );

        Ok(Response::from_string(body)
            .with_status_code(404)
            .with_header(html_header()))
    }
}

fn error_response(e: std::io::Error, path: PathBuf) -> Response<Cursor<Vec<u8>>> {
    let path = path.display();
    let body = format!(
        "<html>
        <body>
        <h1>Error</h1>
        <p>Could not read path: {}</p>
        <p>Path: <code>{}</code></p>
        </body>
        </html>",
        e, path
    );
    error!("Could not handle request: Path: {path}");

    Response::from_string(body)
        .with_status_code(500)
        .with_header(html_header())
}

#[inline]
fn html_header() -> Header {
    Header::from_bytes(&b"Content-Type"[..], &b"text/html"[..]).unwrap()
}

#[cfg(feature = "upload")]
fn destination_path(directory: &Path, filename: &str, behaviour: &DuplicateBehaviour) -> PathBuf {
    let path = directory.join(filename);

    if !path.exists() {
        return path;
    }

    match behaviour {
        DuplicateBehaviour::KeepNew | DuplicateBehaviour::KeepOld => path,
        DuplicateBehaviour::KeepBoth => {
            let stem = Path::new(filename)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy();

            let ext = Path::new(filename).extension().map(|e| e.to_string_lossy());

            let mut i = 1;
            loop {
                let candidate = match &ext {
                    Some(ext) => directory.join(format!("{stem} ({i}).{ext}")),
                    None => directory.join(format!("{stem} ({i})")),
                };

                if !candidate.exists() {
                    return candidate;
                }

                i += 1;
            }
        }
    }
}
