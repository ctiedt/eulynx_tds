use sci_rs::scitds::OccupancyStatus;

#[derive(Clone, Copy)]
pub enum Direction {
    Incoming,
    Outgoing,
}

pub struct Tvps {
    id: String,
    axes_in: u32,
    axes_out: u32,
}

impl Tvps {
    pub fn new<S: Into<String>>(id: S) -> Self {
        Self {
            id: id.into(),
            axes_in: 0,
            axes_out: 0,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn status(&self) -> OccupancyStatus {
        match self.axes_in.cmp(&self.axes_out) {
            std::cmp::Ordering::Less => OccupancyStatus::Disturbed,
            std::cmp::Ordering::Equal => OccupancyStatus::Vacant,
            std::cmp::Ordering::Greater => OccupancyStatus::Occupied,
        }
    }

    pub fn filling_level(&self) -> i32 {
        (self.axes_in as i32) - (self.axes_out as i32)
    }

    pub async fn wait_for_passing_axle(&self) {}
}
