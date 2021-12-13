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
