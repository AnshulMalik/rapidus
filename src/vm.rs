use std::boxed::Box;
use std::collections::HashMap;

use std::cell::RefCell;
use std::rc::Rc;

use bytecode_gen::ByteCode;
use node::BinOp;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Undefined,
    Bool(bool),
    Number(f64),
    String(String),
    Function(usize, Rc<RefCell<HashMap<String, Value>>>),
    NeedThis(Box<Value>),
    WithThis(Box<Value>, Box<Value>),            // Function, This
    EmbeddedFunction(usize), // unknown if usize == 0; specific function if usize > 0
    Object(Rc<RefCell<HashMap<String, Value>>>), // Object(HashMap<String, Value>),
}

// pub struct Value2 {
//     pub kind: u8,
// }

impl Value {
    fn to_string(self) -> String {
        match self {
            Value::String(name) => name,
            Value::Number(n) => format!("{}", n),
            _ => unimplemented!(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConstantTable {
    pub value: Vec<Value>,
    pub string: Vec<String>,
}

impl ConstantTable {
    pub fn new() -> ConstantTable {
        ConstantTable {
            value: vec![],
            string: vec![],
        }
    }
}

macro_rules! label {
    ($name:expr) => {
        unsafe {
            asm!(concat!($name, ":") : : : : "volatile");
        }
    };
}

/// Loads the address of a label and returns it. Parameter $name must be a string and an existing
/// label name.
#[cfg(target_arch = "x86_64")]
macro_rules! label_addr {
    ($name:expr) => {{
        let addr: usize;
        unsafe {
            asm!(concat!("leaq ", $name, "(%rip), $0")
                                                                                  : "=&r"(addr)
                                                                                  :
                                                                                  :
                                                                                  : "volatile" );
        }
        addr
    }};
}

/// Reads the address of the next instruction from the jump table and jumps there.
#[cfg(target_arch = "x86_64")]
macro_rules! dispatch {
    ($pc:expr, $opcode:expr, $jumptable:expr, $counter:expr) => {
        $counter += 1;
        let addr = $jumptable[$opcode as usize];

        unsafe {
            // the inputs of this asm block force these locals to be in the specified
            // registers after $action is exited, so that on entry to the consecutive
            // $action, the previous asm block will be set up with the right register
            // to locals mapping
            asm!("jmpq *$0"
                                 :
                                 : "r"(addr), "{r8d}"($counter), "{ecx}"($opcode), "{rdx}"($pc)
                                 :
                                 : "volatile"
                            );
        }
    };
}

/// Encapsulates a VM instruction between register constraints and dispatches to the
/// next instruction.
///  * $name must be a label name as a string
///  * $pc must be a function-local usize
///  * $opcode must be a function-local u32
///  * $counter must be a function-local integer
///  * $action must be a block containing the VM instruction code
#[cfg(target_arch = "x86_64")]
macro_rules! do_and_dispatch {
    (
        $jumptable:expr, $name:expr, $pc:expr, $opcode:expr, $counter:expr, $action:expr
    ) => {
        // the outputs of this asm block essentially force these locals to
        // be in the specified registers when $action is entered
        unsafe {
            asm!(concat!($name, ":")
                                         : "={r8d}"($counter), "={ecx}"($opcode), "={rdx}"($pc)
                                         :
                                         :
                                         : "volatile");
        }

        {
            $action
        }

        dispatch!($pc, $opcode, $jumptable, $counter);
    };
}

macro_rules! get_int8 {
    ($insts:expr, $pc:expr, $var:ident, $ty:ty) => {
        let $var = $insts[$pc as usize] as $ty;
        $pc += 1;
    };
}

macro_rules! get_int32 {
    ($insts:expr, $pc:expr, $var:ident, $ty:ty) => {
        let $var = (($insts[$pc as usize + 3] as $ty) << 24)
            + (($insts[$pc as usize + 2] as $ty) << 16)
            + (($insts[$pc as usize + 1] as $ty) << 8)
            + ($insts[$pc as usize + 0] as $ty);
        $pc += 4;
    };
}

pub struct VM {
    pub global_objects: Rc<RefCell<HashMap<String, Value>>>,
    pub stack: Vec<Value>,
    pub bp_buf: Vec<usize>,
    pub bp: usize,
    pub sp_history: Vec<usize>,
    pub return_addr: Vec<isize>,
    pub const_table: ConstantTable,
    pub insts: ByteCode,
    pub pc: isize,
    // pub op_table: [fn(&mut VM); 31],
    pub op_table2: [usize; 31],
}

impl VM {
    pub fn new() -> VM {
        let mut obj = HashMap::new();

        obj.insert("console".to_string(), {
            let mut map = HashMap::new();
            map.insert("log".to_string(), Value::EmbeddedFunction(1));
            Value::Object(Rc::new(RefCell::new(map)))
        });

        let global_objects = Rc::new(RefCell::new(obj));

        VM {
            global_objects: global_objects.clone(),
            stack: {
                let mut stack = Vec::with_capacity(128);
                stack.push(Value::Object(global_objects.clone()));
                stack
            },
            bp_buf: Vec::with_capacity(128),
            bp: 0,
            sp_history: Vec::with_capacity(128),
            return_addr: Vec::with_capacity(128),
            const_table: ConstantTable::new(),
            insts: vec![],
            pc: 0isize,
            // op_table: [
            //     end,
            //     create_context,
            //     constract,
            //     create_object,
            //     push_int8,
            //     push_int32,
            //     push_false,
            //     push_true,
            //     push_const,
            //     push_this,
            //     add,
            //     sub,
            //     mul,
            //     div,
            //     rem,
            //     lt,
            //     gt,
            //     le,
            //     ge,
            //     eq,
            //     ne,
            //     get_member,
            //     set_member,
            //     get_global,
            //     set_global,
            //     get_local,
            //     set_local,
            //     jmp_if_false,
            //     jmp,
            //     call,
            //     return_,
            // ],
            op_table2: [
                label_addr!("goto_end"),
                label_addr!("goto_create_context"),
                label_addr!("goto_constract"),
                label_addr!("goto_create_object"),
                label_addr!("goto_push_int8"),
                label_addr!("goto_push_int32"),
                label_addr!("goto_push_false"),
                label_addr!("goto_push_true"),
                label_addr!("goto_push_const"),
                label_addr!("goto_push_this"),
                label_addr!("goto_add"),
                label_addr!("goto_sub"),
                label_addr!("goto_mul"),
                label_addr!("goto_div"),
                label_addr!("goto_rem"),
                label_addr!("goto_lt"),
                label_addr!("goto_gt"),
                label_addr!("goto_le"),
                label_addr!("goto_ge"),
                label_addr!("goto_eq"),
                label_addr!("goto_ne"),
                label_addr!("goto_get_member"),
                label_addr!("goto_set_member"),
                label_addr!("goto_get_global"),
                label_addr!("goto_set_global"),
                label_addr!("goto_get_local"),
                label_addr!("goto_set_local"),
                label_addr!("goto_jmp_if_false"),
                label_addr!("goto_jmp"),
                label_addr!("goto_call"),
                label_addr!("goto_return_"),
            ],
        }
    }
}

pub const END: u8 = 0x00;
pub const CREATE_CONTEXT: u8 = 0x01;
pub const CONSTRACT: u8 = 0x02;
pub const CREATE_OBJECT: u8 = 0x03;
pub const PUSH_INT8: u8 = 0x04;
pub const PUSH_INT32: u8 = 0x05;
pub const PUSH_FALSE: u8 = 0x06;
pub const PUSH_TRUE: u8 = 0x07;
pub const PUSH_CONST: u8 = 0x08;
pub const PUSH_THIS: u8 = 0x09;
pub const ADD: u8 = 0x0a;
pub const SUB: u8 = 0x0b;
pub const MUL: u8 = 0x0c;
pub const DIV: u8 = 0x0d;
pub const REM: u8 = 0x0e;
pub const LT: u8 = 0x0f;
pub const GT: u8 = 0x10;
pub const LE: u8 = 0x11;
pub const GE: u8 = 0x12;
pub const EQ: u8 = 0x13;
pub const NE: u8 = 0x14;
pub const GET_MEMBER: u8 = 0x15;
pub const SET_MEMBER: u8 = 0x16;
pub const GET_GLOBAL: u8 = 0x17;
pub const SET_GLOBAL: u8 = 0x18;
pub const GET_LOCAL: u8 = 0x19;
pub const SET_LOCAL: u8 = 0x1a;
pub const JMP_IF_FALSE: u8 = 0x1b;
pub const JMP: u8 = 0x1c;
pub const CALL: u8 = 0x1d;
pub const RETURN: u8 = 0x1e;

impl VM {
    pub fn run(&mut self, insts: ByteCode) {
        self.insts = insts;
        self.do_run2();
        // println!("stack trace: {:?}", self.stack);
    }

    pub fn do_run(&mut self) {
        loop {
            // println!("inst: {} - {}", self.insts[self.pc as usize], self.pc);
            let code = self.insts[self.pc as usize];
            // self.op_table[code as usize](self);
            if code == RETURN || code == END {
                break;
            }
            // println!("stack trace: {:?} - {}", self.stack, *pc);
        }
    }

    #[inline(never)]
    pub fn do_run2(&mut self) {
        let mut pc = 0;
        let mut opcode = self.insts[pc as usize] as u32;
        let mut counter = 0;
        println!("here");
        dispatch!(pc, opcode, self.op_table2, counter);

        do_and_dispatch!(
            self.op_table2,
            "goto_create_context",
            pc,
            opcode,
            counter,
            {
                println!("pc: {}", pc);
                pc += 1; // create_context
                get_int32!(self.insts, pc, n, usize);
                get_int32!(self.insts, pc, argc, usize);
                println!("{} {} ", n, argc);
                self.bp_buf.push(self.bp);
                self.sp_history.push(self.stack.len() - argc);
                self.bp = self.stack.len() - argc;
                for _ in 0..n {
                    self.stack.push(Value::Undefined);
                }
                opcode = self.insts[pc as usize] as u32;
            }
        );

        do_and_dispatch!(self.op_table2, "goto_constract", pc, opcode, counter, {
            pc += 1; // constract
            get_int32!(self.insts, pc, argc, usize);

            let mut callee = self.stack.pop().unwrap();

            loop {
                match callee {
                    Value::Function(dst, _) => {
                        self.return_addr.push(pc);

                        // insert new 'this'
                        let pos = self.stack.len() - argc;
                        let new_this = Rc::new(RefCell::new(HashMap::new()));
                        self.stack.insert(pos, Value::Object(new_this.clone()));

                        pc = dst as isize;
                        self.do_run();
                        self.stack.pop(); // return value by func
                        self.stack.push(Value::Object(new_this));
                        break;
                    }
                    Value::NeedThis(callee_) => {
                        callee = *callee_;
                    }
                    Value::WithThis(callee_, _this) => {
                        callee = *callee_;
                    }
                    c => {
                        println!("Call: err: {:?}, pc = {}", c, pc);
                        break;
                    }
                }
            }
            opcode = self.insts[pc as usize] as u32;
        });

        do_and_dispatch!(self.op_table2, "goto_push_int8", pc, opcode, counter, {
            pc += 1; // push_int
            get_int8!(self.insts, pc, n, i32);
            self.stack.push(Value::Number(n as f64));
            opcode = self.insts[pc as usize] as u32;
        });

        do_and_dispatch!(self.op_table2, "goto_push_int32", pc, opcode, counter, {
            pc += 1; // push_int
            get_int32!(self.insts, pc, n, i32);
            self.stack.push(Value::Number(n as f64));
            opcode = self.insts[pc as usize] as u32;
        });

        do_and_dispatch!(self.op_table2, "goto_push_false", pc, opcode, counter, {
            pc += 1; // push_false
            self.stack.push(Value::Bool(false));
            opcode = self.insts[pc as usize] as u32;
        });

        do_and_dispatch!(self.op_table2, "goto_push_true", pc, opcode, counter, {
            pc += 1; // push_true
            self.stack.push(Value::Bool(true));
            opcode = self.insts[pc as usize] as u32;
        });

        do_and_dispatch!(self.op_table2, "goto_push_const", pc, opcode, counter, {
            pc += 1; // push_const
            get_int32!(self.insts, pc, n, usize);
            self.stack.push(self.const_table.value[n].clone());
            opcode = self.insts[pc as usize] as u32;
        });

        do_and_dispatch!(self.op_table2, "goto_push_this", pc, opcode, counter, {
            pc += 1; // push_this
            let val = self.stack[self.bp].clone();
            self.stack.push(val);
            opcode = self.insts[pc as usize] as u32;
        });

        macro_rules! bin_op {
            ($name:ident, $name2:expr, $binop:ident) => {
                do_and_dispatch!(self.op_table2, $name2, pc, opcode, counter, {
                    pc += 1; // $name
                    binary(self, &BinOp::$binop);
                    opcode = self.insts[pc as usize] as u32;
                });
            };
        }

        bin_op!(add, "goto_add", Add);
        bin_op!(sub, "goto_sub", Sub);
        bin_op!(mul, "goto_mul", Mul);
        bin_op!(div, "goto_div", Div);
        bin_op!(rem, "goto_rem", Rem);
        bin_op!(lt, "goto_lt", Lt);
        bin_op!(gt, "goto_gt", Gt);
        bin_op!(le, "goto_le", Le);
        bin_op!(ge, "goto_ge", Ge);
        bin_op!(eq, "goto_eq", Eq);
        bin_op!(ne, "goto_ne", Ne);

        do_and_dispatch!(self.op_table2, "goto_get_member", pc, opcode, counter, {
            pc += 1; // get_global
            let member = self.stack.pop().unwrap().to_string();
            let parent = self.stack.pop().unwrap();
            match parent {
                Value::Object(map)
                | Value::Function(_, map)
                | Value::NeedThis(box Value::Function(_, map)) => {
                    match map.borrow().get(member.as_str()) {
                        Some(addr) => {
                            let val = addr.clone();
                            if let Value::NeedThis(callee) = val {
                                self.stack.push(Value::WithThis(
                                    callee,
                                    Box::new(Value::Object(map.clone())),
                                ))
                            } else {
                                self.stack.push(val)
                            }
                        }
                        None => self.stack.push(Value::Undefined),
                    }
                }
                _ => unreachable!(),
            }
            opcode = self.insts[pc as usize] as u32;
        });

        do_and_dispatch!(self.op_table2, "goto_set_member", pc, opcode, counter, {
            pc += 1; // get_global
            let member = self.stack.pop().unwrap().to_string();
            let parent = self.stack.pop().unwrap();
            let val = self.stack.pop().unwrap();
            match parent {
                Value::Object(map)
                | Value::Function(_, map)
                | Value::NeedThis(box Value::Function(_, map)) => {
                    *map.borrow_mut()
                        .entry(member)
                        .or_insert_with(|| Value::Undefined) = val;
                }
                e => unreachable!("{:?}", e),
            }
            opcode = self.insts[pc as usize] as u32;
        });

        do_and_dispatch!(self.op_table2, "goto_get_global", pc, opcode, counter, {
            pc += 1; // get_global
            get_int32!(self.insts, pc, n, usize);
            let val = (*(*self.global_objects)
                .borrow()
                .get(self.const_table.string[n].as_str())
                .unwrap())
                .clone();
            self.stack.push(val);
            opcode = self.insts[pc as usize] as u32;
        });

        do_and_dispatch!(self.op_table2, "goto_set_global", pc, opcode, counter, {
            pc += 1; // set_global
            get_int32!(self.insts, pc, n, usize);
            *(*self.global_objects)
                .borrow_mut()
                .entry(self.const_table.string[n].clone())
                .or_insert_with(|| Value::Undefined) = self.stack.pop().unwrap();
            opcode = self.insts[pc as usize] as u32;
        });

        do_and_dispatch!(self.op_table2, "goto_get_local", pc, opcode, counter, {
            pc += 1; // get_local
            get_int32!(self.insts, pc, n, usize);
            let val = self.stack[self.bp + n].clone();
            self.stack.push(val);
            opcode = self.insts[pc as usize] as u32;
        });

        do_and_dispatch!(self.op_table2, "goto_set_local", pc, opcode, counter, {
            pc += 1; // set_local
            get_int32!(self.insts, pc, n, usize);
            let val = self.stack.pop().unwrap();
            self.stack[self.bp + n] = val;
            opcode = self.insts[pc as usize] as u32;
        });

        do_and_dispatch!(self.op_table2, "goto_jmp", pc, opcode, counter, {
            pc += 1; // jmp
            get_int32!(self.insts, pc, dst, i32);
            pc += dst as isize;
            opcode = self.insts[pc as usize] as u32;
        });

        do_and_dispatch!(self.op_table2, "goto_jmp_if_false", pc, opcode, counter, {
            pc += 1; // jmp_if_false
            get_int32!(self.insts, pc, dst, i32);
            let cond = self.stack.pop().unwrap();
            if let Value::Bool(false) = cond {
                pc += dst as isize
            }
            opcode = self.insts[pc as usize] as u32;
        });

        do_and_dispatch!(self.op_table2, "goto_call", pc, opcode, counter, {
            pc += 1; // Call
            get_int32!(self.insts, pc, argc, usize);

            let mut this = None;

            let mut callee = self.stack.pop().unwrap();

            loop {
                match callee {
                    Value::EmbeddedFunction(1) => {
                        let mut args = vec![];
                        for _ in 0..argc {
                            args.push(self.stack.pop().unwrap());
                        }
                        args.reverse();
                        console_log(args);
                        break;
                    }
                    Value::Function(dst, _) => {
                        self.return_addr.push(pc);
                        if let Some(this) = this {
                            let pos = self.stack.len() - argc;
                            self.stack.insert(pos, this);
                        }
                        pc = dst as isize;
                        // self.do_run();
                        break;
                    }
                    Value::NeedThis(callee_) => {
                        this = Some(Value::Object(self.global_objects.clone()));
                        callee = *callee_;
                    }
                    Value::WithThis(callee_, this_) => {
                        this = Some(*this_);
                        callee = *callee_;
                    }
                    c => {
                        println!("Call: err: {:?}, pc = {}", c, pc);
                        break;
                    }
                }
            }

            // EmbeddedFunction(1)
            fn console_log(args: Vec<Value>) {
                let args_len = args.len();
                for i in 0..args_len {
                    match args[i] {
                        Value::String(ref s) => print!("{}", s),
                        Value::Number(ref n) => print!("{}", n),
                        Value::Undefined => print!("undefined"),
                        _ => {}
                    }
                    if args_len - 1 != i {
                        print!(" ")
                    }
                }
                println!()
            }
            opcode = self.insts[pc as usize] as u32;
        });

        do_and_dispatch!(self.op_table2, "goto_return_", pc, opcode, counter, {
            let val = self.stack.pop().unwrap();
            let former_sp = self.sp_history.pop().unwrap();
            self.stack.truncate(former_sp);
            self.stack.push(val);
            pc = self.return_addr.pop().unwrap();
            self.bp = self.bp_buf.pop().unwrap();
            opcode = self.insts[pc as usize] as u32;
        });

        do_and_dispatch!(self.op_table2, "goto_create_object", pc, opcode, counter, {
            pc += 1; // create_context
            get_int32!(self.insts, pc, len, usize);

            let mut map = HashMap::new();
            for _ in 0..len {
                let name = if let Value::String(name) = self.stack.pop().unwrap() {
                    name
                } else {
                    panic!()
                };
                let val = self.stack.pop().unwrap();
                map.insert(name, val.clone());
            }
            self.stack.push(Value::Object(Rc::new(RefCell::new(map))));
            opcode = self.insts[pc as usize] as u32;
        });

        label!("goto_end");
    }
}

// #[inline]
// fn end(_self: &mut VM) {}
//
// #[inline]
// fn create_context(self_: &mut VM) {
//     self_.pc += 1; // create_context
//     get_int32!(self_, n, usize);
//     get_int32!(self_, argc, usize);
//     self_.bp_buf.push(self_.bp);
//     self_.sp_history.push(self_.stack.len() - argc);
//     self_.bp = self_.stack.len() - argc;
//     for _ in 0..n {
//         self_.stack.push(Value::Undefined);
//     }
// }
//
// #[inline]
// fn constract(self_: &mut VM) {
//     self_.pc += 1; // constract
//     get_int32!(self_, argc, usize);
//
//     let mut callee = self_.stack.pop().unwrap();
//
//     loop {
//         match callee {
//             Value::Function(dst, _) => {
//                 self_.return_addr.push(self_.pc);
//
//                 // insert new 'this'
//                 let pos = self_.stack.len() - argc;
//                 let new_this = Rc::new(RefCell::new(HashMap::new()));
//                 self_.stack.insert(pos, Value::Object(new_this.clone()));
//
//                 self_.pc = dst as isize;
//                 self_.do_run();
//                 self_.stack.pop(); // return value by func
//                 self_.stack.push(Value::Object(new_this));
//                 break;
//             }
//             Value::NeedThis(callee_) => {
//                 callee = *callee_;
//             }
//             Value::WithThis(callee_, _this) => {
//                 callee = *callee_;
//             }
//             c => {
//                 println!("Call: err: {:?}, pc = {}", c, self_.pc);
//                 break;
//             }
//         }
//     }
// }
//
// #[inline]
// fn create_object(self_: &mut VM) {
//     self_.pc += 1; // create_context
//     get_int32!(self_, len, usize);
//
//     let mut map = HashMap::new();
//     for _ in 0..len {
//         let name = if let Value::String(name) = self_.stack.pop().unwrap() {
//             name
//         } else {
//             panic!()
//         };
//         let val = self_.stack.pop().unwrap();
//         map.insert(name, val.clone());
//     }
//     self_.stack.push(Value::Object(Rc::new(RefCell::new(map))));
// }
//
// #[inline]
// fn push_int8(self_: &mut VM) {
//     self_.pc += 1; // push_int
//     get_int8!(self_, n, i32);
//     self_.stack.push(Value::Number(n as f64));
// }
//
// #[inline]
// fn push_int32(self_: &mut VM) {
//     self_.pc += 1; // push_int
//     get_int32!(self_, n, i32);
//     self_.stack.push(Value::Number(n as f64));
// }
//
// #[inline]
// fn push_false(self_: &mut VM) {
//     self_.pc += 1; // push_false
//     self_.stack.push(Value::Bool(false));
// }
//
// #[inline]
// fn push_true(self_: &mut VM) {
//     self_.pc += 1; // push_true
//     self_.stack.push(Value::Bool(true));
// }
//
// #[inline]
// fn push_const(self_: &mut VM) {
//     self_.pc += 1; // push_const
//     get_int32!(self_, n, usize);
//     self_.stack.push(self_.const_table.value[n].clone());
// }
//
// #[inline]
// fn push_this(self_: &mut VM) {
//     self_.pc += 1; // push_this
//     let val = self_.stack[self_.bp].clone();
//     self_.stack.push(val);
// }
//
// macro_rules! bin_op {
//     ($name:ident, $binop:ident) => {
//         #[inline]
//         fn $name(self_: &mut VM) {
//             self_.pc += 1; // $name
//             binary(self_, &BinOp::$binop);
//         }
//     };
// }
//
// bin_op!(add, Add);
// bin_op!(sub, Sub);
// bin_op!(mul, Mul);
// bin_op!(div, Div);
// bin_op!(rem, Rem);
// bin_op!(lt, Lt);
// bin_op!(gt, Gt);
// bin_op!(le, Le);
// bin_op!(ge, Ge);
// bin_op!(eq, Eq);
// bin_op!(ne, Ne);
//
#[inline(never)]
fn binary(self_: &mut VM, op: &BinOp) {
    let rhs = self_.stack.pop().unwrap();
    let lhs = self_.stack.pop().unwrap();
    match (lhs, rhs) {
        (Value::Number(n1), Value::Number(n2)) => self_.stack.push(match op {
            &BinOp::Add => Value::Number(n1 + n2),
            &BinOp::Sub => Value::Number(n1 - n2),
            &BinOp::Mul => Value::Number(n1 * n2),
            &BinOp::Div => Value::Number(n1 / n2),
            &BinOp::Rem => Value::Number((n1 as i64 % n2 as i64) as f64),
            &BinOp::Lt => Value::Bool(n1 < n2),
            &BinOp::Gt => Value::Bool(n1 > n2),
            &BinOp::Le => Value::Bool(n1 <= n2),
            &BinOp::Ge => Value::Bool(n1 >= n2),
            &BinOp::Eq => Value::Bool(n1 == n2),
            &BinOp::Ne => Value::Bool(n1 != n2),
            _ => panic!(),
        }),
        (Value::String(s1), Value::Number(n2)) => self_.stack.push(match op {
            &BinOp::Add => {
                let concat = format!("{}{}", s1, n2);
                Value::String(concat)
            }
            _ => panic!(),
        }),
        (Value::Number(n1), Value::String(s2)) => self_.stack.push(match op {
            &BinOp::Add => {
                let concat = format!("{}{}", n1, s2);
                Value::String(concat)
            }
            _ => panic!(),
        }),
        (Value::String(s1), Value::String(s2)) => self_.stack.push(match op {
            &BinOp::Add => {
                let concat = format!("{}{}", s1, s2);
                Value::String(concat)
            }
            _ => panic!(),
        }),
        _ => {}
    }
}
//
// #[inline]
// fn get_member(self_: &mut VM) {
//     self_.pc += 1; // get_global
//     let member = self_.stack.pop().unwrap().to_string();
//     let parent = self_.stack.pop().unwrap();
//     match parent {
//         Value::Object(map)
//         | Value::Function(_, map)
//         | Value::NeedThis(box Value::Function(_, map)) => match map.borrow().get(member.as_str()) {
//             Some(addr) => {
//                 let val = addr.clone();
//                 if let Value::NeedThis(callee) = val {
//                     self_.stack.push(Value::WithThis(
//                         callee,
//                         Box::new(Value::Object(map.clone())),
//                     ))
//                 } else {
//                     self_.stack.push(val)
//                 }
//             }
//             None => self_.stack.push(Value::Undefined),
//         },
//         _ => unreachable!(),
//     }
// }
//
// #[inline]
// fn set_member(self_: &mut VM) {
//     self_.pc += 1; // get_global
//     let member = self_.stack.pop().unwrap().to_string();
//     let parent = self_.stack.pop().unwrap();
//     let val = self_.stack.pop().unwrap();
//     match parent {
//         Value::Object(map)
//         | Value::Function(_, map)
//         | Value::NeedThis(box Value::Function(_, map)) => {
//             *map.borrow_mut()
//                 .entry(member)
//                 .or_insert_with(|| Value::Undefined) = val;
//         }
//         e => unreachable!("{:?}", e),
//     }
// }
//
// #[inline]
// fn get_global(self_: &mut VM) {
//     self_.pc += 1; // get_global
//     get_int32!(self_, n, usize);
//     let val = (*(*self_.global_objects)
//         .borrow()
//         .get(self_.const_table.string[n].as_str())
//         .unwrap())
//         .clone();
//     self_.stack.push(val);
// }
//
// #[inline]
// fn set_global(self_: &mut VM) {
//     self_.pc += 1; // set_global
//     get_int32!(self_, n, usize);
//     *(*self_.global_objects)
//         .borrow_mut()
//         .entry(self_.const_table.string[n].clone())
//         .or_insert_with(|| Value::Undefined) = self_.stack.pop().unwrap();
// }
//
// #[inline]
// fn get_local(self_: &mut VM) {
//     self_.pc += 1; // get_local
//     get_int32!(self_, n, usize);
//     let val = self_.stack[self_.bp + n].clone();
//     self_.stack.push(val);
// }
//
// #[inline]
// fn set_local(self_: &mut VM) {
//     self_.pc += 1; // set_local
//     get_int32!(self_, n, usize);
//     let val = self_.stack.pop().unwrap();
//     self_.stack[self_.bp + n] = val;
// }
//
// #[inline]
// fn jmp(self_: &mut VM) {
//     self_.pc += 1; // jmp
//     get_int32!(self_, dst, i32);
//     self_.pc += dst as isize;
// }
//
// #[inline]
// fn jmp_if_false(self_: &mut VM) {
//     self_.pc += 1; // jmp_if_false
//     get_int32!(self_, dst, i32);
//     let cond = self_.stack.pop().unwrap();
//     if let Value::Bool(false) = cond {
//         self_.pc += dst as isize
//     }
// }
//
// #[inline]
// fn call(self_: &mut VM) {
//     self_.pc += 1; // Call
//     get_int32!(self_, argc, usize);
//
//     let mut this = None;
//
//     let mut callee = self_.stack.pop().unwrap();
//
//     loop {
//         match callee {
//             Value::EmbeddedFunction(1) => {
//                 let mut args = vec![];
//                 for _ in 0..argc {
//                     args.push(self_.stack.pop().unwrap());
//                 }
//                 args.reverse();
//                 console_log(args);
//                 break;
//             }
//             Value::Function(dst, _) => {
//                 self_.return_addr.push(self_.pc);
//                 if let Some(this) = this {
//                     let pos = self_.stack.len() - argc;
//                     self_.stack.insert(pos, this);
//                 }
//                 self_.pc = dst as isize;
//                 self_.do_run();
//                 break;
//             }
//             Value::NeedThis(callee_) => {
//                 this = Some(Value::Object(self_.global_objects.clone()));
//                 callee = *callee_;
//             }
//             Value::WithThis(callee_, this_) => {
//                 this = Some(*this_);
//                 callee = *callee_;
//             }
//             c => {
//                 println!("Call: err: {:?}, pc = {}", c, self_.pc);
//                 break;
//             }
//         }
//     }
//
//     // EmbeddedFunction(1)
//     fn console_log(args: Vec<Value>) {
//         let args_len = args.len();
//         for i in 0..args_len {
//             match args[i] {
//                 Value::String(ref s) => print!("{}", s),
//                 Value::Number(ref n) => print!("{}", n),
//                 Value::Undefined => print!("undefined"),
//                 _ => {}
//             }
//             if args_len - 1 != i {
//                 print!(" ")
//             }
//         }
//         println!()
//     }
// }
//
// #[inline]
// fn return_(self_: &mut VM) {
//     let val = self_.stack.pop().unwrap();
//     let former_sp = self_.sp_history.pop().unwrap();
//     self_.stack.truncate(former_sp);
//     self_.stack.push(val);
//     self_.pc = self_.return_addr.pop().unwrap();
//     self_.bp = self_.bp_buf.pop().unwrap();
// }

// #[rustfmt::skip]
// pub fn vm2_test() {
//     let mut vm2 = VM::new();
//     vm2.const_table.value.push(Value::Function(41, Rc::new(RefCell::new(HashMap::new()))));
//     vm2.const_table.value.push(Value::String("log".to_string()));
//     vm2.const_table.string.push("console".to_string());
//
//     // Loop for 100,000,000
//     // AllocLocalVar(1, 1)
//     // Push(Number(0.0))
//     // SetLocal(1)
//     // GetLocal(1)
//     // Push(Number(100000000.0))
//     // Lt
//     // JmpIfFalse(6)
//     // GetLocal(1)
//     // Push(Number(1.0))
//     // Add
//     // SetLocal(1)
//     // Jmp(-8)
//     // End
//     // vm2.run(vec![
//     //         CREATE_CONTEXT, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, // CreateContext 1, 1
//     //         PUSH_INT32, 0x00, 0x00, 0x00, 0x00, // PushInt 0
//     //         SET_LOCAL, 0x01, 0x00, 0x00, 0x00, // SetLocal 1
//     //         GET_LOCAL, 0x01, 0x00, 0x00, 0x00, // GetLocal 1
//     //         PUSH_INT32, 0x00, 0xe1, 0xf5, 0x05, // PushInt 100,000,000
//     //         LT, // Lt
//     //         JMP_IF_FALSE, 0x15, 0x00, 0x00, 0x00, // JmpIfFalse 21
//     //         GET_LOCAL, 0x01, 0x00, 0x00, 0x00, // GetLocal 1
//     //         PUSH_INT32, 0x01, 0x00, 0x00, 0x00, // PushInt 1
//     //         ADD, // Add
//     //         SET_LOCAL, 0x01, 0x00, 0x00, 0x00, // SetLocal 1
//     //         JMP, 0xdb, 0xff, 0xff, 0xff, // Jmp -37
//     //         END, // End
//     // ]);
//
//     // Fibo 10
//     // AllocLocalVar(0, 1)
//     // Push(Number(10.0))
//     // Push(Function(5, RefCell { value: {} }))
//     // Call(1)
//     // End
//     // AllocLocalVar(0, 1)
//     // GetLocal(0)
//     // Push(Number(2.0))
//     // Lt
//     // JmpIfFalse(3)
//     // Push(Number(1.0))
//     // Return
//     // GetLocal(0)
//     // Push(Number(1.0))
//     // Sub
//     // Push(Function(5, RefCell { value: {} }))
//     // Call(1)
//     // GetLocal(0)
//     // Push(Number(2.0))
//     // Sub
//     // Push(Function(5, RefCell { value: {} }))
//     // Call(1)
//     // Add
//     // Return
//     vm2.run(vec![
//         CREATE_CONTEXT, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, // CreateContext 1, 1
//         PUSH_INT32, 35,0,0,0, // PushInt 10
//         PUSH_CONST, 0x00, 0x00, 0x00, 0x00, // PushConst 0
//         CALL, 0x01, 0x00, 0x00, 0x00, // Call 1
//         GET_GLOBAL, 0x00, 0x00, 0x00, 0x00, // GetGlobal 0 (console)
//         PUSH_CONST, 0x01, 0x00, 0x00, 0x00, // PushConst 1 (log)
//         GET_MEMBER, // GetMember
//         CALL, 0x01, 0x00, 0x00, 0x00, // Call 1
//         END, // End
//         CREATE_CONTEXT, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, // CreateContext 0, 1
//         GET_LOCAL, 0x00, 0x00, 0x00, 0x00, // GetLocal 0
//         PUSH_INT32, 0x02, 0,0,0,// PushInt 2
//         LT, // Lt
//         JMP_IF_FALSE, 6, 0x00, 0x00, 0x00, // JmpIfFalse 6
//         PUSH_INT32, 0x01,0,0,0, // PushInt 1
//         RETURN, // Return
//         GET_LOCAL, 0x00, 0x00, 0x00, 0x00, // GetLocal 0
//         PUSH_INT32, 0x01,0,0,0, // PushInt 1
//         SUB, // Sub
//         PUSH_CONST, 0x00, 0x00, 0x00, 0x00, // PushConst 0
//         CALL, 0x01, 0x00, 0x00, 0x00, // Call 1
//         GET_LOCAL, 0x00, 0x00, 0x00, 0x00, // GetLocal 0
//         PUSH_INT32, 0x02, 0,0,0,// PushInt 2
//         SUB, // Sub
//         PUSH_CONST, 0x00, 0x00, 0x00, 0x00, // PushConst 0
//         CALL, 0x01, 0x00, 0x00, 0x00, // Call 1
//         ADD, // Add
//         RETURN, // Return
//     ]);
// }
