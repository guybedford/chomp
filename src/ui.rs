// Chomp Task Runner
// Copyright (C) 2022  Guy Bedford

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use std::io::{stdout, Write};
use crossterm::{
    ExecutableCommand, QueueableCommand, Result,
    execute, queue, terminal, cursor, style::{self, Stylize}
};

pub struct ChompUI {
}

impl ChompUI {
  pub fn new (tty: bool) -> ChompUI {
    ChompUI {}
  }

  pub fn create_box (&self) -> Result<()> {
    let mut stdout = stdout();
    let (cols, rows) = terminal::size()?;

    // stdout.execute(terminal::Clear(terminal::ClearType::FromCursorUp))?;

    queue!(stdout,
      cursor::SavePosition,
    );
    
    for y in 0..rows - 1 {
        for x in 0..cols - 1 {
        if (y == 0 || y == rows - 1) || (x == 0 || x == cols - 1) {
            // in this loop we are more efficient by not flushing the buffer.
            queue!(
              stdout,
              cursor::MoveTo(x, y),
              style::PrintStyledContent("█".magenta())
            )?;
        }
        }
    }

    execute!(
      stdout,
      cursor::MoveTo(0, 0),
      style::PrintStyledContent("█".red())
    )?;

    execute!(stdout, cursor::RestorePosition)?;
    Ok(())
  }
}
