use crate::{
    cursor::Pos as CursorPos,
    framebuffer::FrameBuffer,
    future::Either,
    io::ps2::Ps2Keyboard,
    sleep::{self, WakeupRequester},
    time::MonotonicTime,
    util::updated_val::UpdatedVal,
};

use core::ops::{Add, AddAssign};

use alloc::vec::Vec;

const DELTA: f32 = 0.03;
const PADDLE_Y: f32 = 0.8;
const PADDLE_WIDTH: f32 = 0.1;
const PADDLE_HEIGHT: f32 = 0.02;
const PADDLE_VEL: f32 = 0.03;
const BALL_VEL: f32 = 0.03;
const BALL_RAD: f32 = 0.01;

const BRICK_WIDTH: f32 = PADDLE_WIDTH;
const BRICK_HEIGHT: f32 = PADDLE_HEIGHT;
const BRICK_TOP_Y: f32 = 0.1;
const BRICK_SPACING: f32 = 0.005;
const BRICK_COLS: u32 = 5;
const BRICK_ROWS: u32 = 5;

//const W_DOWN: u8 = 0x11;
//const W_UP: u8 = 0x91;
const A_DOWN: u8 = 0x1e;
const A_UP: u8 = 0x9e;
//const S_DOWN: u8 = 0x1f;
//const S_UP: u8 = 0x9f;
const D_DOWN: u8 = 0x20;
const D_UP: u8 = 0xa0;
//space -> 0x39  0xb9

#[derive(Copy, Clone)]
struct Pos2d {
    x: f32,
    y: f32,
}

impl Pos2d {
    fn new(x: f32, y: f32) -> Pos2d {
        Pos2d { x, y }
    }
}

impl Add<Pos2d> for Pos2d {
    type Output = Pos2d;

    fn add(mut self, rhs: Pos2d) -> Self::Output {
        self += rhs;
        self
    }
}

impl AddAssign<Pos2d> for Pos2d {
    fn add_assign(&mut self, rhs: Pos2d) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl core::ops::Mul<f32> for Pos2d {
    type Output = Pos2d;

    fn mul(mut self, rhs: f32) -> Self::Output {
        self.x *= rhs;
        self.y *= rhs;

        self
    }
}

struct State {
    pause: bool,
    paddle_position: f32,
    paddle_dir: i8,
    ball_position: Pos2d,
    ball_direction: Pos2d,
    bricks: Vec<Pos2d>,
}

pub struct Game<'a> {
    state: State,
    framebuffer: &'a mut FrameBuffer,
    ps2: &'a mut Ps2Keyboard,
    monotonic_time: &'a MonotonicTime,
    wakeup_list: &'a WakeupRequester,
    cursor_pos: UpdatedVal<CursorPos>,
}

impl<'a> Game<'a> {
    pub fn new(
        framebuffer: &'a mut FrameBuffer,
        ps2: &'a mut Ps2Keyboard,
        monotonic_time: &'a MonotonicTime,
        wakeup_list: &'a WakeupRequester,
        cursor_pos: UpdatedVal<CursorPos>,
    ) -> Game<'a> {
        let state = State {
            pause: false,
            paddle_position: 0.5,
            paddle_dir: 0,
            ball_position: Pos2d::new(0.5, 0.5),
            ball_direction: Pos2d::new(0.5, 0.5),
            bricks: gen_bricks(),
        };

        Game {
            framebuffer,
            ps2,
            monotonic_time,
            wakeup_list,
            state,
            cursor_pos,
        }
    }

    fn draw_paddle(&mut self) {
        let paddle_box = Rect {
            center: Pos2d::new(self.state.paddle_position, PADDLE_Y),
            size: Pos2d::new(PADDLE_WIDTH, PADDLE_HEIGHT),
        };

        draw_rect(&paddle_box, &[1.0, 1.0, 1.0], self.framebuffer)
    }

    fn handle_input(&mut self, input: Option<Input>) {
        match input {
            Some(Input::Keyboard(D_DOWN)) => {
                self.state.paddle_dir = 1;
            }
            Some(Input::Keyboard(D_UP)) => {
                if self.state.paddle_dir == 1 {
                    self.state.paddle_dir = 0;
                }
            }
            Some(Input::Keyboard(A_DOWN)) => {
                self.state.paddle_dir = -1;
            }
            Some(Input::Keyboard(A_UP)) => {
                if self.state.paddle_dir == -1 {
                    self.state.paddle_dir = 0;
                }
            }
            Some(Input::Keyboard(185)) => {
                self.state.pause = !self.state.pause;
            }

            Some(Input::Mouse(pos)) => {
                self.state.paddle_position = pos.x;
                self.state.paddle_dir = 0;
            }
            _ => (),
        }
    }

    fn physics(&mut self) {
        update_paddle_position(&mut self.state.paddle_position, self.state.paddle_dir);

        update_ball_position(
            &mut self.state.ball_position,
            &mut self.state.ball_direction,
            &Pos2d::new(self.state.paddle_position, PADDLE_Y),
        );

        update_bricks(
            &mut self.state.bricks,
            &self.state.ball_position,
            &mut self.state.ball_direction,
        );
    }

    fn clear_screen(&mut self) {
        let fb_color = self.framebuffer.convert_color(0.0, 0.0, 0.0);
        for y in 0..self.framebuffer.height() {
            for x in 0..self.framebuffer.width() {
                self.framebuffer.set_pixel(y, x, fb_color);
            }
        }
    }

    fn draw(&mut self) {
        self.clear_screen();
        self.draw_paddle();
        draw_ball(&self.state.ball_position, self.framebuffer);
        draw_bricks(&self.state.bricks, self.framebuffer);
    }

    fn update(&mut self, input: Option<Input>) {
        // When input happens, update game state
        // Otherwise, update game state and sleep
        self.handle_input(input);

        if self.state.pause {
            return;
        }

        self.physics();
        self.draw();
    }

    pub async fn run(&mut self) {
        let mut input = None;
        loop {
            let start = self.monotonic_time.get();
            // Input handling goes here
            self.update(core::mem::take(&mut input));

            let next_frame_time = start + (DELTA * self.monotonic_time.tick_freq()) as usize;

            let now = self.monotonic_time.get();
            let remaining_s = (next_frame_time - now) as f32 / self.monotonic_time.tick_freq();
            let mut sleep_fut = core::pin::pin!(sleep::sleep(
                remaining_s,
                self.monotonic_time,
                self.wakeup_list
            ));

            loop {
                let input_fut = core::pin::pin!(wait_for_input(self.ps2, &self.cursor_pos));

                match crate::future::select(input_fut, sleep_fut).await {
                    Either::Left((found_input, next_sleep_fut)) => {
                        input = Some(found_input);
                        sleep_fut = next_sleep_fut;
                    }
                    Either::Right((_, _)) => {
                        break;
                    }
                }
            }
        }
    }
}

fn update_paddle_position(paddle_position: &mut f32, paddle_dir: i8) {
    *paddle_position += paddle_dir as f32 * PADDLE_VEL;

    let max_paddle_pos = 1.0 - PADDLE_WIDTH / 2.0;
    let min_paddle_pos = PADDLE_WIDTH / 2.0;
    *paddle_position = paddle_position.clamp(min_paddle_pos, max_paddle_pos);
}

fn update_ball_position(
    ball_position: &mut Pos2d,
    ball_direction: &mut Pos2d,
    paddle_position: &Pos2d,
) {
    *ball_position += *ball_direction * BALL_VEL;

    if ball_position.x > 1.0 {
        assert!(ball_direction.x > 0.0);
        ball_position.x = 1.0;
        ball_direction.x *= -1.0;
    }

    if ball_position.y > 1.0 {
        assert!(ball_direction.y > 0.0);
        ball_position.y = 1.0;
        ball_direction.y *= -1.0;
    }

    if ball_position.x < 0.0 {
        assert!(ball_direction.x < 0.0);
        ball_position.x = 0.0;
        ball_direction.x *= -1.0;
    }

    if ball_position.y < 0.0 {
        assert!(ball_direction.y < 0.0);
        ball_position.y = 0.0;
        ball_direction.y *= -1.0;
    }

    let ball_collision = ball_collision_box(ball_position);

    let paddle_rect = Rect {
        center: *paddle_position,
        size: Pos2d::new(PADDLE_WIDTH, PADDLE_HEIGHT),
    };

    if rect_intersects(&ball_collision, &paddle_rect) {
        ball_direction.y *= -1.0;
        ball_position.y = paddle_position.y - PADDLE_HEIGHT / 2.0;
    }
}

fn draw_ball(ball_position: &Pos2d, framebuffer: &mut FrameBuffer) {
    let ball_rect = ball_collision_box(ball_position);
    draw_rect(&ball_rect, &[1.0, 1.0, 1.0], framebuffer);
}

struct Rect {
    center: Pos2d,
    size: Pos2d,
}

impl Rect {
    fn right(&self) -> f32 {
        self.center.x + self.size.x / 2.0
    }

    fn left(&self) -> f32 {
        self.center.x - self.size.x / 2.0
    }

    fn top(&self) -> f32 {
        self.center.y - self.size.y / 2.0
    }

    fn bottom(&self) -> f32 {
        self.center.y + self.size.y / 2.0
    }
}

fn rect_intersects(a: &Rect, b: &Rect) -> bool {
    !(a.left() > b.right() || b.left() > a.right() || a.top() > b.bottom() || b.top() > a.bottom())
}

fn gen_bricks() -> Vec<Pos2d> {
    let mut ret = Vec::new();
    const BRICK_GRID_WIDTH: f32 = (BRICK_WIDTH + BRICK_SPACING) * BRICK_COLS as f32 - BRICK_SPACING;
    const BRICK_LEFT_X: f32 = 0.5 - BRICK_GRID_WIDTH / 2.0 + BRICK_WIDTH / 2.0;
    for y in 0..BRICK_ROWS {
        let y_offs = BRICK_TOP_Y + (BRICK_HEIGHT + BRICK_SPACING) * y as f32;
        for x in 0..BRICK_COLS {
            // 0 -> 1
            let x_offs = BRICK_LEFT_X + (BRICK_WIDTH + BRICK_SPACING) * x as f32;

            let brick = Pos2d::new(x_offs, y_offs);
            ret.push(brick);
        }
    }
    ret
}

fn ball_collision_box(ball_position: &Pos2d) -> Rect {
    Rect {
        center: *ball_position,
        size: Pos2d::new(BALL_RAD * 2.0, BALL_RAD * 2.0),
    }
}

fn update_bricks(bricks: &mut Vec<Pos2d>, ball_position: &Pos2d, ball_direction: &mut Pos2d) {
    let ball_rect = ball_collision_box(ball_position);
    let mut collision_idx = None;
    for (i, brick) in bricks.iter().enumerate() {
        let brick_box = Rect {
            center: *brick,
            size: Pos2d::new(BRICK_WIDTH, BRICK_HEIGHT),
        };

        if rect_intersects(&ball_rect, &brick_box) {
            collision_idx = Some(i);
            ball_direction.y *= -1.0;
            break;
        }
    }

    if let Some(i) = collision_idx {
        bricks.swap_remove(i);
    }
}

fn draw_bricks(bricks: &[Pos2d], framebuffer: &mut FrameBuffer) {
    for brick in bricks {
        let brick_box = Rect {
            center: *brick,
            size: Pos2d::new(BRICK_WIDTH, BRICK_HEIGHT),
        };
        let color = [0.0, 0.0, 1.0];
        draw_rect(&brick_box, &color, framebuffer);
    }
}

fn draw_rect(rect: &Rect, color: &[f32; 3], framebuffer: &mut FrameBuffer) {
    let min_x =
        ((rect.left() * framebuffer.width() as f32) as u32).clamp(0, framebuffer.width() - 1);
    let max_x =
        ((rect.right() * framebuffer.width() as f32) as u32).clamp(0, framebuffer.width() - 1);
    let min_y =
        ((rect.top() * framebuffer.height() as f32) as u32).clamp(0, framebuffer.height() - 1);
    let max_y =
        ((rect.bottom() * framebuffer.height() as f32) as u32).clamp(0, framebuffer.height() - 1);

    let fb_color = framebuffer.convert_color(color[0], color[1], color[2]);

    for y in min_y..max_y {
        for x in min_x..max_x {
            framebuffer.set_pixel(y, x, fb_color);
        }
    }
}

enum Input {
    Keyboard(u8),
    Mouse(CursorPos),
}

async fn wait_for_input(ps2: &mut Ps2Keyboard, mouse: &UpdatedVal<CursorPos>) -> Input {
    let fut1 = core::pin::pin!(ps2.read());
    let fut2 = core::pin::pin!(mouse.wait());

    let input = crate::future::select(fut1, fut2).await;

    match input {
        Either::Left((keyboard_input, _)) => Input::Keyboard(keyboard_input),
        Either::Right((mouse_input, _)) => Input::Mouse(mouse_input),
    }
}
