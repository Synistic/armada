use serde::{Deserialize, Serialize};

use crate::vec::Vec2;

/// Everything that changes the sim from outside goes through here: player
/// input, AI, scripts, network. That makes AI and multiplayer the same problem.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    /// Move in formation at the speed of the slowest ship. Ships fire at
    /// whatever comes into arc but do not break formation.
    Move { ships: Vec<u32>, to: Vec2 },
    /// Like `Move`, but ships break off to fight anything they meet.
    AttackMove { ships: Vec<u32>, to: Vec2 },
    Attack { ships: Vec<u32>, target: u32 },
    /// Hold position, keep firing, turn guns toward the nearest enemy.
    Stop { ships: Vec<u32> },
    /// Back to the autonomous stance: hunt the nearest enemy.
    Engage { ships: Vec<u32> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimedCommand {
    pub tick: u32,
    pub team: u8,
    pub command: Command,
}

/// Commands sorted by tick. Order inside a tick is insertion order, and every
/// peer must insert in the same order (e.g. by player slot).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandLog {
    pub commands: Vec<TimedCommand>,
}

impl CommandLog {
    pub fn push(&mut self, cmd: TimedCommand) {
        let pos = self.commands.partition_point(|c| c.tick <= cmd.tick);
        self.commands.insert(pos, cmd);
    }

    pub fn for_tick(&self, tick: u32) -> &[TimedCommand] {
        let start = self.commands.partition_point(|c| c.tick < tick);
        let end = self.commands.partition_point(|c| c.tick <= tick);
        &self.commands[start..end]
    }
}
