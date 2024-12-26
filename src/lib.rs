mod error;
use std::{
    fs::File,
    io::{self, BufRead, BufReader, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering::Relaxed},
        mpsc::{self, Receiver},
        Arc,
    },
    thread::{self, sleep},
    time::{Duration, Instant},
};
use temp_dir::TempDir;

pub use error::Error;

#[derive(Debug, PartialEq, Eq, Clone)]
#[non_exhaustive]
pub struct Conf<'a> {
    /// command line arguments
    pub args: Vec<&'a str>,

    /// Try to spawn the process `attempt` time
    ///
    /// The OS is giving available ports to use, however, they aren't booked, so it could rarely
    /// happen they are used at the time the process is spawn. When retrying other available ports
    /// are returned reducing the probability of conflicts to negligible.
    attempts: u8,

    /// The ip to bind to
    pub ip: Option<String>,

    /// The port to listen on
    pub port: Option<u16>,

    // Path to the binary
    pub binary: Option<String>,
}

impl<'a> Default for Conf<'a> {
    fn default() -> Self {
        Self {
            args: Vec::new(),
            attempts: 5,
            ip: None,
            port: None,
            binary: None,
        }
    }
}

/// Returns a non-used local port if available.
///
/// Note there is a race condition during the time the method check availability and the caller
pub fn get_available_port() -> Result<u16, Error> {
    // using 0 as port let the system assign a port available
    let t = TcpListener::bind(("127.0.0.1", 0))?; // 0 means the OS choose a free port
    Ok(t.local_addr().map(|s| s.port())?)
}

/// Struct representing the electrs process with related information
pub struct NostrD {
    /// Process child handle, used to terminate the process when this struct is dropped
    pub process: Child,
    /// Work directory, removed when dropped
    pub work_dir: TempDir,
    /// A buffer receiving stdout and stderr
    pub logs: Receiver<String>,
    /// The port we listen to
    pub port: u16,
    /// the address we listen to
    pub addr: String,
    /// Path to the binary
    pub binary: PathBuf,
}

fn try_read_line<R: BufRead>(reader: &mut R) -> io::Result<Option<String>> {
    let mut buffer = Vec::new();
    // Try to read until a newline
    match reader.read_until(b'\n', &mut buffer)? {
        0 => Ok(None), // EOF reached or no data available
        _ => {
            if let Ok(line) = String::from_utf8(buffer) {
                Ok(Some(line))
            } else {
                Ok(None)
            }
        }
    }
}

impl NostrD {
    /// Create a new nostr process
    pub fn new() -> Result<NostrD, Error> {
        NostrD::with_conf(&Conf::default())
    }

    /// Create a new process using given [Conf]
    pub fn with_conf(conf: &Conf) -> Result<NostrD, Error> {
        let mut args = conf.args.clone();
        let ip = conf.ip.clone().unwrap_or("127.0.0.1".into());
        let port = conf.port.unwrap_or(get_available_port()?);

        let file_path = file!();
        let mut bin_dir = Path::new(file_path)
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        bin_dir.push("bin");
        bin_dir.push("nostr-rs-relay_0_9_0_linux");

        let bin = if let Some(bin) = conf.binary.clone() {
            bin
        } else if let Some(bin) = &bin_dir.to_str() {
            bin.to_string()
        } else {
            panic!("no valid binary path")
        };

        let exe = Path::new(&bin);
        if !exe.exists() {
            panic!("path {:?} does not exists!", exe);
        }
        if !exe.is_file() {
            panic!(" path {:?} is not a file!", exe);
        }

        // create the temp dir
        let work_dir = TempDir::with_prefix("nostrd_").unwrap();

        // config file
        let mut file = File::create(work_dir.child("config.toml"))?;
        writeln!(&file, "[network]").unwrap();
        writeln!(file, "address = \"{}\"", ip.clone()).unwrap();
        writeln!(file, "port = \"{}\"", port).unwrap();
        drop(file);

        // config
        args.push("--config");
        let cfg_path = work_dir.child("config.toml");
        let path = cfg_path.as_path().to_str().expect("hardcoded");
        args.push(path);

        // db location
        args.push("--db");
        args.push(work_dir.path().to_str().expect("hardcoded"));

        let (sender, logs) = mpsc::channel();

        std::env::set_var("RUST_LOG", "debug");
        let mut p = None;
        #[allow(clippy::never_loop)]
        'f: for _ in 0..conf.attempts {
            let mut process = Command::new(exe)
                .args(args.clone())
                .stderr(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()?;
            let timeout = Instant::now() + Duration::from_secs(3);
            let stdout = process.stdout.take().unwrap();
            let mut stdout_reader = BufReader::new(stdout);
            let s = sender.clone();
            let stop = Arc::new(AtomicBool::new(false));
            let stop2 = stop.clone();
            thread::spawn(move || loop {
                if let Ok(Some(line)) = try_read_line(&mut stdout_reader) {
                    let _ = s.send(line);
                } else if stop2.load(Relaxed) {
                    break;
                }
            });

            loop {
                if Instant::now() > timeout {
                    let _ = process.kill();
                    stop.store(true, Relaxed);
                } else if let Ok(log) = logs.try_recv() {
                    if log.contains("control message listener started") {
                        p = Some(process);
                        break 'f;
                    } else {
                        sleep(Duration::from_millis(10));
                    }
                }
            }
        }
        let mut process = if let Some(p) = p {
            p
        } else {
            panic!("Fail to start NostrD after {} attempts", conf.attempts);
        };
        let stderr = process.stderr.take().unwrap();

        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                sender.send(line.unwrap()).unwrap();
            }
        });

        Ok(NostrD {
            process,
            work_dir,
            logs,
            addr: ip.clone(),
            port,
            binary: exe.to_path_buf(),
        })
    }

    /// Return the current workdir path of the running electrs
    pub fn workdir(&self) -> PathBuf {
        self.work_dir.path().to_path_buf()
    }

    /// terminate the process
    pub fn kill(&mut self) -> Result<(), Error> {
        self.inner_kill()?;
        // Wait for the process to exit
        match self.process.wait() {
            Ok(_) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// clear the log buffer
    pub fn clear_logs(&mut self) {
        while self.logs.try_recv().is_ok() {}
    }

    fn inner_kill(&mut self) -> Result<(), Error> {
        // Send SIGINT signal to electrsd
        Ok(nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(self.process.id() as i32),
            nix::sys::signal::SIGINT,
        )?)
    }

    pub fn url(&self) -> String {
        format!("ws://{}:{}", self.addr, self.port)
    }
}

impl Drop for NostrD {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}
