use crate::util::{
    async_channel::{self, Receiver, Sender},
    updated_val::UpdatedVal,
};

pub struct Movement {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Copy, Clone)]
pub struct Pos {
    pub x: f32,
    pub y: f32,
}

pub struct Cursor {
    rx: Receiver<Movement>,
    tx: Sender<Movement>,
    pos: UpdatedVal<Pos>,
}

impl Cursor {
    pub fn new() -> Cursor {
        let (tx, rx) = async_channel::channel();
        let pos = UpdatedVal::new(Pos { x: 0.5, y: 0.5 });
        Cursor { rx, tx, pos }
    }

    pub fn get_movement_writer(&self) -> Sender<Movement> {
        self.tx.clone()
    }

    pub fn get_pos_reader(&self) -> UpdatedVal<Pos> {
        self.pos.clone()
    }

    pub async fn service(&mut self) {
        loop {
            let movement = self.rx.recv().await;
            let mut pos = self.pos.read().await;
            pos.x += movement.x;
            pos.x = pos.x.clamp(0.0, 1.0);
            pos.y += movement.y;
            pos.y = pos.y.clamp(0.0, 1.0);
            debug!("cursor pos: {:?}", pos);
            self.pos.write(pos).await;
        }
    }
}
