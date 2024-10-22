use clap::{Parser, Subcommand};
use rustyline::{error::ReadlineError, DefaultEditor};
use shellwords::split;
use vfs::Vfs;

#[derive(Parser, Debug)]
#[command(no_binary_name = true)]
#[command(disable_help_flag = true)]
#[command(override_usage = "<COMMAND> [ARGS]")]
struct Args {
    #[command(subcommand)]
    commands: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Output information about a file (file descriptor data).
    Stat {
        /// hard link name
        name: String,
    },
    /// Output a list of hard links to files with file descriptor numbers in a directory
    #[clap(name = "ls")]
    List {
        /// hard link name
        #[clap(default_value = "/")]
        name: String,
    },
    /// Create a regular file and create a hard link named name to it in the directory
    Create {
        /// hard link name
        name: String,
    },
    /// Open a regular file pointed to by the hard link named name
    Open {
        /// hard link name
        name: String,
    },
    /// Close previously opened file with numeric file descriptor
    Close {
        /// file descriptor number
        fd: usize,
    },
    /// Specify the offset for the open file where the next read or write will begin
    Seek {
        /// file descriptor number
        fd: usize,
        /// offset
        offset: usize,
    },
    /// Read size bytes of data from an open file, size is added to the offset value
    Read {
        /// file descriptor number
        fd: usize,
        /// number of bytes to read
        size: usize,
    },
    /// Write size bytes of data to an open file, size is added to the offset value
    Write {
        /// file descriptor number
        fd: usize,
        /// data to write
        data: String,
    },
    /// Create a hard link named name2 to the file pointed to by the hard link named name1
    Link {
        /// hard link name1
        name1: String,
        /// hard link name2
        name2: String,
    },
    /// Remove the hard link named name
    Unlink {
        /// hard link name
        name: String,
    },
    /// Change the size of the file pointed to by the hard link named name
    Truncate {
        /// hard link name
        name: String,
        /// size
        size: usize,
    },
    /// Exit the program
    Exit,
}

fn main() {
    let mut editor = DefaultEditor::new().unwrap();
    let mut vfs = Vfs::new();
    let mut interupted = false;
    println!(
        "Welcome to VFS {}.\nType \"help\" for more information",
        env!("CARGO_PKG_VERSION")
    );
    loop {
        match editor.readline("$ ") {
            Ok(line) => {
                let input = match split(&line) {
                    Ok(input) => input,
                    Err(_) => {
                        eprintln!("error: unterminated quote found");
                        continue;
                    }
                };
                if input.is_empty() {
                    continue;
                }
                match Args::try_parse_from(input) {
                    Ok(args) => match args.commands {
                        Commands::Exit => {
                            break;
                        }
                        Commands::Stat { name } => {
                            match vfs.stat(&name) {
                                Ok(statx) => println!("{}", statx),
                                Err(err) => eprintln!("{}", err),
                            };
                        }
                        Commands::List { name } => match vfs.ls(&name) {
                            Ok(names) => {
                                for name in names {
                                    println!("{}", name);
                                }
                            }
                            Err(err) => eprintln!("{}", err),
                        },
                        Commands::Create { name } => {
                            if let Err(err) = vfs.create(&name) {
                                eprintln!("{}", err);
                            }
                        }
                        Commands::Link { name1, name2 } => {
                            if let Err(err) = vfs.link(&name1, &name2) {
                                eprintln!("{}", err);
                            }
                        }
                        Commands::Unlink { name } => {
                            if let Err(err) = vfs.unlink(&name) {
                                eprintln!("{}", err);
                            }
                        }
                        Commands::Open { name } => match vfs.open(&name) {
                            Ok(fd) => println!("{}", fd),
                            Err(err) => eprintln!("{}", err),
                        },
                        Commands::Close { fd } => {
                            if let Err(err) = vfs.close(fd) {
                                eprintln!("{}", err);
                            }
                        }
                        Commands::Seek { fd, offset } => {
                            if let Err(err) = vfs.seek(fd, offset) {
                                eprintln!("{}", err);
                            }
                        }
                        Commands::Write { fd, data } => match vfs.write(fd, data.as_bytes()) {
                            Ok(size) => println!("{}", size),
                            Err(err) => eprintln!("{}", err),
                        },
                        Commands::Read { fd, size } => match vfs.read(fd, size) {
                            Ok(data) => println!("{}", String::from_utf8_lossy(&data)),
                            Err(err) => eprintln!("{}", err),
                        },
                        Commands::Truncate { name, size } => {
                            if let Err(err) = vfs.truncate(&name, size) {
                                eprintln!("{}", err);
                            }
                        }
                    },
                    Err(err) => {
                        eprint!("{}", err);
                    }
                }
                editor.add_history_entry(line).unwrap();
            }
            Err(ReadlineError::Interrupted) => {
                if interupted {
                    break;
                }
                println!("(To exit, press Ctrl+C again or Ctrl+D or type \"exit\")");
                interupted = true;
                continue;
            }
            Err(ReadlineError::Eof) => {
                break;
            }
            Err(err) => {
                eprintln!("error: {}", err);
                break;
            }
        }
        interupted = false;
    }
}
