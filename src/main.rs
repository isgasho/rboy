#[crate_id = "rboy"];

extern mod extra;
extern mod sdl;

use cpu::CPU;
use extra::getopts;
use extra::comm::DuplexStream;
use extra::arc::RWArc;

mod register;
mod mbc;
mod mmu;
mod cpu;
mod serial;
mod timer;
mod keypad;
mod gpu;
mod sound;

fn main() {
	let args: ~[~str] = std::os::args();
	let program = args[0].clone() + " <filename>";

	let opts = ~[ getopts::groups::optflag("s", "serial", "Output serial to stdout") ];
	let matches = match getopts::groups::getopts(args.tail(), opts) {
		Ok(m) => { m }
		Err(f) => { println!("{}", f.to_err_msg()); return }
	};

	let filename: ~str = if !matches.free.is_empty() {
		matches.free[0].clone()
	} else {
		println!("{}", getopts::groups::usage(program, opts));
		return;
	};

	sdl::init([sdl::InitVideo]);
	sdl::wm::set_caption("RBoy - A gameboy in Rust", "rboy");
	let screen = match sdl::video::set_video_mode(160*2, 144*2, 32, [sdl::video::HWSurface], [sdl::video::DoubleBuf]) {
		Ok(screen) => screen,
		Err(err) => fail!("failed to open screen: {}", err),
	};

	let (sdlstream, cpustream) = DuplexStream::new();
	let rawscreen = ~[0x00u8,.. 160*144*3];
	let arc = RWArc::new(rawscreen);
	let arc2 = arc.clone();
	do spawn { cpuloop(&cpustream, arc2, filename, &matches); }

	let mut timer = std::io::timer::Timer::new().unwrap();
	let periodic = timer.periodic(8);

	'main : loop {
		periodic.recv();
		match sdlstream.try_recv() {
			Some(_) => recalculate_screen(screen, &arc),
			None => {},
		}
		'event : loop {
			match sdl::event::poll_event() {
				sdl::event::QuitEvent => break 'main,
				sdl::event::NoEvent => break 'event,
				sdl::event::KeyEvent(sdl::event::EscapeKey, _, _, _)
					=> break 'main,
				sdl::event::KeyEvent(sdl::event::LShiftKey, true, _, _)
					=> sdlstream.send(SpeedUp),
				sdl::event::KeyEvent(sdl::event::LShiftKey, false, _, _)
					=> sdlstream.send(SlowDown),
				sdl::event::KeyEvent(sdlkey, true, _, _) => {
					match sdl_to_keypad(sdlkey) {
						Some(key) => sdlstream.send(KeyDown(key)),
						None => {},
					}
				},
				sdl::event::KeyEvent(sdlkey, false, _, _) => {
					match sdl_to_keypad(sdlkey) {
						Some(key) => sdlstream.send(KeyUp(key)),
						None => {},
					}
				},
				_ => {}
			}
		}
	}
	sdlstream.try_send(Poweroff);
}

fn sdl_to_keypad(key: sdl::event::Key) -> Option<keypad::KeypadKey> {
	match key {
		sdl::event::ZKey => Some(keypad::A),
		sdl::event::XKey => Some(keypad::B),
		sdl::event::UpKey => Some(keypad::Up),
		sdl::event::DownKey => Some(keypad::Down),
		sdl::event::LeftKey => Some(keypad::Left),
		sdl::event::RightKey => Some(keypad::Right),
		sdl::event::SpaceKey => Some(keypad::Select),
		sdl::event::ReturnKey => Some(keypad::Start),
		_ => None,
	}
}

fn recalculate_screen(screen: &sdl::video::Surface, arc: &RWArc<~[u8]>) {
	arc.read(|data| 
		for y in range(0, 144) {
			for x in range(0, 160) {
				screen.fill_rect(
					Some(sdl::Rect { x: (x*2) as i16, y: (y*2) as i16, w: 2, h: 2 }),
					sdl::video::RGB(data[y*160*3 + x*3 + 0],
					                data[y*160*3 + x*3 + 1],
					                data[y*160*3 + x*3 + 2])
				);
			}
		}
	);
	screen.flip();
}

enum GBEvent {
	KeyUp(keypad::KeypadKey),
	KeyDown(keypad::KeypadKey),
	Poweroff,
	SpeedUp,
	SlowDown,
}

fn cpuloop(channel: &DuplexStream<uint, GBEvent>, arc: RWArc<~[u8]>, filename: ~str, matches: &getopts::Matches) {
	let mut c = CPU::new(filename);
	c.mmu.serial.enabled = matches.opt_present("serial");

	let mut timer = std::io::timer::Timer::new().unwrap();
	let mut periodic = timer.periodic(8);

	let waitticks = (4194.304 * 4.0) as uint;

	let mut ticks = 0;
	loop {
		while ticks < waitticks {
			ticks += c.cycle();
			if c.mmu.gpu.updated {
				c.mmu.gpu.updated = false;
				arc.write(|data|
					for i in range(0, data.len()) { data[i] = c.mmu.gpu.data[i]; }
				);
				channel.try_send(0);
			}
		}
		ticks -= waitticks;
		periodic.recv();

		match channel.try_recv() {
			None => {},
			Some(Poweroff) => { break; },
			Some(KeyUp(key)) => c.mmu.keypad.keyup(key),
			Some(KeyDown(key)) => c.mmu.keypad.keydown(key),
			Some(SpeedUp) => periodic = timer.periodic(1),
			Some(SlowDown) => periodic = timer.periodic(8),
		};
	}
}