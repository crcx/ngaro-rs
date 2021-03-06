use std::fs::File;
use std::path::Path;
use std::cmp::max;
use std::ops::{Deref, DerefMut};

use byteorder::{LittleEndian, ReadBytesExt};

use self::Action::*;

macro_rules! get_memory {
    ($foo: ident, $addr: expr) => {
        match $foo.memory.memory_space.get($addr as usize) {
            Some(&x) => x,
            None => return None,
        }
    };
    ($foo: ident, $addr: expr, $rval: expr) => {
        match $foo.memory.memory_space.get($addr as usize) {
            Some(&x) => x,
            None => return $rval,
        }
    };
}

struct Memory {
    data_stack: Vec<i32>,
    address_stack: Vec<i32>,
    memory_space: Vec<i32>,
}

impl Memory {
    fn new(path: &Path) -> Memory {
        let mut file = File::open(path).unwrap();
        let size = file.metadata().unwrap().len() / 4;
        let mut memory = Vec::with_capacity(max(size as usize, 1024*1024)); // 4MB or image size

        loop {
            match file.read_i32::<LittleEndian>() {
                Ok(x) => memory.push(x),
                Err(::byteorder::Error::UnexpectedEOF) => break,
                Err(e) => panic!(e),
            }
        }

        while memory.capacity() > memory.len() {
            memory.push(0);
        }

        Memory {
            data_stack: Vec::new(),
            address_stack: Vec::new(),
            memory_space: memory
        }
    }
}

pub struct Ports<'a> {
    ports: &'a mut [i32]
}

impl<'a> Drop for Ports<'a> {
    fn drop(&mut self) {
        self.ports.get_mut(0).map(|x| *x = 1);
    }
}

impl<'a> Deref for Ports<'a> {
    type Target = [i32];
    fn deref(&self) -> &[i32] {
        &*self.ports
    }
}

impl<'a> DerefMut for Ports<'a> {
    fn deref_mut(&mut self) -> &mut [i32] {
        self.ports
    }
}

pub struct Info {
    pub memory_size: i32,
    pub data_stack_depth: i32,
    pub address_stack_depth: i32,
}

pub struct CPU {
    memory: Memory,
    ip: i32,
    ports: [i32; 12]
}

impl CPU {
    pub fn new(path: &Path) -> CPU {
        CPU {
            memory: Memory::new(path),
            ip: 0,
            ports: [0; 12],
        }
    }

    pub fn ports_and_stack<'a>(&'a mut self) -> (Ports<'a>, &'a mut Vec<i32>) {
        let ports = Ports { ports: &mut self.ports };
        let stack = &mut self.memory.data_stack;
        (ports, stack)
    }

    pub fn get_info(&self) -> Info {
        Info {
            memory_size: self.memory.memory_space.len() as i32,
            data_stack_depth: self.memory.data_stack.len() as i32,
            address_stack_depth: self.memory.address_stack.len() as i32,
        }
    }

    pub fn pop_data(&mut self) -> i32 {
        self.memory.data_stack.pop().expect("Data stack underflow.")
    }

    pub fn push_data(&mut self, data: i32) {
        self.memory.data_stack.push(data)
    }

    fn pop_address(&mut self) -> i32 {
        self.memory.address_stack.pop().expect("Address stack underflow.")
    }

    fn push_address(&mut self, data: i32) {
        self.memory.address_stack.push(data)
    }

    fn jump(&mut self) {
        self.ip += 1;
        self.ip = get_memory!(self, self.ip, panic!("Jump out of bounds.")) - 1;
    }

    fn cond_stack_jump<F>(&mut self, cond: F)
        where F: Fn(i32, i32) -> bool
    {
        let (a, b) = (self.pop_data(), self.pop_data());
        if cond(a,b) { self.jump() } else { self.ip += 1; }
    }

    fn pop_2_push_1<F>(&mut self, func: F)
        where F: FnOnce(i32, i32) -> i32
    {
        let (a, b) = (self.pop_data(), self.pop_data());
        self.push_data(func(a,b));
    }

    pub fn next(&mut self) -> Option<Action> {
        loop {
            let instruction = get_memory!(self, self.ip);
            match instruction {
                0 => { } // NOP
                1 => { // LIT X
                    self.ip += 1;
                    let data = get_memory!(self, self.ip);
                    self.push_data(data);
                }
                2 => { // DUP
                    let item = self.pop_data();
                    self.push_data(item);
                    self.push_data(item);
                }
                3 => { // DROP
                    self.pop_data();
                }
                4 => { // SWAP
                    let (a, b) = (self.pop_data(), self.pop_data());
                    self.push_data(a);
                    self.push_data(b);
                }
                5 => { // PUSH
                    let data = self.pop_data();
                    self.push_address(data);
                }
                6 => { // POP
                    let data = self.pop_address();
                    self.push_data(data);
                }
                7 => { // LOOP A
                    let mut data = self.pop_data();
                    data -= 1;
                    if data > 0 {
                        self.jump();
                        self.push_data(data);
                    } else {
                        self.ip += 1;
                    }
                }
                8 => { // JUMP A
                    self.jump();
                }
                9 => { // RETURN
                    let addr = self.pop_address();
                    self.ip = addr;
                }
                10 => { // GT_JUMP
                    self.cond_stack_jump(|a, b| b > a);
                }
                11 => { // LT_JUMP
                    self.cond_stack_jump(|a, b| b < a);
                }
                12 => { // NE_JUMP
                    self.cond_stack_jump(|a, b| a != b);
                }
                13 => { // EQ_JUMP
                    self.cond_stack_jump(|a, b| a == b);
                }
                14 => { // FETCH
                    let addr = self.pop_data();
                    let data = *self.memory.memory_space.get(addr as usize).expect("FETCH beyond bounds.");
                    self.push_data(data);
                }
                15 => { // STORE
                    let (addr, data) = (self.pop_data(), self.pop_data());
                    *self.memory.memory_space.get_mut(addr as usize).expect("STORE beyond bounds.") = data;
                }
                16 => { // ADD
                    self.pop_2_push_1(|a, b| a+b);
                }
                17 => { // SUBTRACT
                    self.pop_2_push_1(|a, b| b-a);
                }
                18 => { // MULTIPLY
                    self.pop_2_push_1(|a, b| a*b);
                }
                19 => { // DIVMOD
                    let (a, b) = (self.pop_data(), self.pop_data());
                    self.push_data(b % a);
                    self.push_data(b / a);
                }
                20 => { // AND
                    self.pop_2_push_1(|a, b| a&b);
                }
                21 => { // OR
                    self.pop_2_push_1(|a, b| a|b);
                }
                22 => { // XOR
                    self.pop_2_push_1(|a, b| a^b);
                }
                23 => { // SHL
                    self.pop_2_push_1(|a, b| b<<(a as usize));
                }
                24 => { // SHR
                    self.pop_2_push_1(|a, b| (b as u32>>(a as usize)) as i32);
                }
                25 => { // ZERO_EXIT
                    let data = self.pop_data();
                    if data == 0 {
                        self.ip = self.pop_address();
                    } else {
                        self.push_data(data);
                    }
                }
                26 => { // INC
                    let data = self.pop_data();
                    self.push_data(data+1);
                }
                27 => { // DEC
                    let data = self.pop_data();
                    self.push_data(data-1);
                }
                28 => { // IN
                    let port = self.pop_data();
                    let data = self.ports.get(port as usize).map_or(0, |&x| x);
                    self.push_data(data);
                    self.ports.get_mut(port as usize).map(|x| *x = 0);
                }
                29 => { // OUT
                    let (port, data) = (self.pop_data(), self.pop_data());
                    self.ports.get_mut(port as usize).map(|x| *x = data);
                }
                30 => { // WAIT
                    if self.ports.get(0).map_or(false, |&x| x == 0) {
                        return Some(Wait);
                    }
                }
                x => { // Implicit call
                    let ip = self.ip;
                    self.push_address(ip);
                    self.ip = x - 1;
                }
            };
            self.ip += 1;
        }
    }
}

pub enum Action {
    Empty,
    Wait,
}

#[allow(dead_code)]
mod debug {
    pub fn opcode_to_name(opcode: i32) -> &'static str {
        const NAMES: &'static [&'static str] = &[
            "NOP",
            "LIT",
            "DUP",
            "DROP",
            "SWAP",
            "PUSH",
            "POP",
            "LOOP",
            "JUMP",
            "RETURN",
            "LT_JUMP",
            "GT_JUMP",
            "NE_JUMP",
            "EQ_JUMP",
            "FETCH",
            "STORE",
            "ADD",
            "SUBTRACT",
            "MULTIPLY",
            "DIVMOD",
            "AND",
            "OR",
            "XOR",
            "SHL",
            "SHR",
            "ZERO_EXIT",
            "INC",
            "DEC",
            "IN",
            "OUT",
            "WAIT",
                ];
        NAMES.get(opcode as usize).map_or("CALL", |&x| x)
    }
}
