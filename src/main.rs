extern crate rapidus;
use rapidus::bytecode_gen;
use rapidus::extract_anony_func;
use rapidus::fv_finder;
use rapidus::fv_solver;
use rapidus::lexer;
use rapidus::parser;
use rapidus::vm;
use rapidus::vm_codegen;

extern crate clap;
use clap::{App, Arg};

extern crate nix;
use nix::sys::wait::*;
use nix::unistd::*;

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::prelude::*;

const VERSION_STR: &'static str = env!("CARGO_PKG_VERSION");

fn main() {
    let app = App::new("Rapidus")
        .version(VERSION_STR)
        .author("uint256_t")
        .about("A toy JavaScript engine")
        .arg(
            Arg::with_name("debug")
                .help("Show useful information for debugging")
                .long("debug"),
        )
        .arg(Arg::with_name("file").help("Input file name").index(1));
    let app_matches = app.clone().get_matches();

    if let Some(filename) = app_matches.value_of("file") {
        if !app_matches.is_present("debug") {
            run(filename);
            return;
        }

        let mut file_body = String::new();

        match OpenOptions::new().read(true).open(filename) {
            Ok(mut ok) => ok
                .read_to_string(&mut file_body)
                .ok()
                .expect("cannot read file"),
            Err(e) => {
                println!("error: {}", e);
                return;
            }
        };

        let mut lexer = lexer::Lexer::new(file_body.clone());

        println!("Lexer:");
        while let Ok(token) = lexer.next() {
            println!("{:?}", token);
        }

        let mut parser = parser::Parser::new(file_body);

        println!("Parser:");
        let mut node = parser.parse_all();
        println!("{:?}", node);

        extract_anony_func::AnonymousFunctionExtractor::new().run_toplevel(&mut node);
        fv_finder::FreeVariableFinder::new().run_toplevel(&mut node);
        println!("extract_anony_func, fv_finder:\n {:?}", node);
        fv_solver::FreeVariableSolver::new().run_toplevel(&mut node);

        println!("extract_anony_func, fv_finder, fv_solver:\n {:?}", node);

        let mut vm_codegen = vm_codegen::VMCodeGen::new();
        let mut insts = vec![];
        let mut func_addr_in_bytecode_and_its_entity = HashMap::new();
        vm_codegen.compile(&node, &mut insts, &mut func_addr_in_bytecode_and_its_entity);

        bytecode_gen::show(&insts);

        // println!("Result:");
        // let mut vm = vm::VM::new();
        // vm.global_objects.extend(vm_codegen.global_varmap);
        // vm.run(insts);

        // println!("VM CodeGen Test:");
        // vm_codegen::test();
    }
}

fn run(file_name: &str) {
    match fork() {
        Ok(ForkResult::Parent { child, .. }) => match waitpid(child, None) {
            Ok(ok) => match ok {
                WaitStatus::Exited(_, status) => if status != 0 {
                    println!("exited. status: {}", status)
                },
                WaitStatus::Signaled(pid, status, _) => {
                    // We can do anything (like calling destructors) here.
                    println!("child: pid={:?}, status={:?}", pid, status);
                    println!("Rapidus Internal Error: segmentation fault");
                }
                e => panic!("Rapidus Internal Error: VM exited abnormally!: {:?}", e),
            },
            Err(e) => panic!("Rapidus Internal Error: waitpid failed: {:?}", e),
        },
        Ok(ForkResult::Child) => {
            let mut file_body = String::new();

            match OpenOptions::new().read(true).open(file_name) {
                Ok(mut ok) => match ok.read_to_string(&mut file_body).ok() {
                    Some(x) => x,
                    None => {
                        panic!("error: cannot read file");
                    }
                },
                Err(e) => {
                    println!("error: {}", e);
                    return;
                }
            };

            let mut parser = parser::Parser::new(file_body);

            let mut node = parser.parse_all();

            extract_anony_func::AnonymousFunctionExtractor::new().run_toplevel(&mut node);
            fv_finder::FreeVariableFinder::new().run_toplevel(&mut node);
            fv_solver::FreeVariableSolver::new().run_toplevel(&mut node);

            let mut vm_codegen = vm_codegen::VMCodeGen::new();
            let mut insts = vec![];
            let mut func_addr_in_bytecode_and_its_entity = HashMap::new();
            vm_codegen.compile(
                &node,
                &mut insts,
                &mut func_addr_in_bytecode_and_its_entity,
            );

            // bytecode_gen::show(&insts);

            println!("Result:");

            // println!("{:?}", insts);

            let mut vm = vm::VM::new();
            vm.const_table = vm_codegen.bytecode_gen.const_table;
            (*vm.global_objects)
                .borrow_mut()
                .extend(vm_codegen.global_varmap);
            vm.run(insts);
        }
        Err(e) => panic!("Rapidus Internal Error: fork failed: {:?}", e),
    }
}
