# frozen_string_literal: true

require 'io/console'
require 'pty'
require 'timeout'

# Runs a command in a pseudo terminal and keeps a model of its screen, so tests can type keys
# and read what a user would see. The model knows the escape sequences ratatui sends through
# crossterm: cursor moves, clearing, styles (ignored, the raw output keeps them), and the cursor
# position query, which it answers the way a terminal would.
class TerminalSession
  TIMEOUT = 5

  attr_reader :rows, :columns, :pid

  def initialize(command, env: {}, rows: 24, columns: 80, start_row: 1)
    @rows = rows
    @columns = columns
    @grid = Array.new(rows) { Array.new(columns, ' ') }
    @row = start_row - 1
    @column = 0
    @raw = +''
    @output = +''
    @mutex = Mutex.new
    @reader, @writer, @pid = PTY.spawn(env, 'sh', '-c', %(stty rows #{rows} cols #{columns} && exec "$0" "$@"),
                                       *command)
    @pump = Thread.new { pump }
  end

  def type(text)
    @writer.write(text)
    @writer.flush
  end

  def press(*keys)
    keys.each { |key| type(KEYS.fetch(key)) }
  end

  KEYS = {
    up: "\e[A",
    down: "\e[B",
    page_up: "\e[5~",
    page_down: "\e[6~",
    enter: "\r",
    tab: "\t",
    escape: "\e",
    backspace: "\x7F",
    ctrl_c: "\x03",
    ctrl_d: "\x04",
    ctrl_n: "\x0E",
    ctrl_p: "\x10",
    ctrl_u: "\x15",
  }.freeze

  TOKEN = /\A(?:\e\[([?\d;:]*)([@-~])|\e[^\[]|[\r\n\b]|[^\e\r\n\b])/m

  # The screen as lines with trailing spaces removed
  def screen
    @mutex.synchronize { @grid.map { |row| row.join.rstrip } }
  end

  # Everything the command wrote, escape sequences included
  def output
    @mutex.synchronize { @output.dup }
  end

  def wait_for(timeout: TIMEOUT)
    Timeout.timeout(timeout) do
      sleep(0.01) until yield(screen)
    end
  rescue Timeout::Error
    raise Timeout::Error, "screen did not reach the expected state:\n#{screen.join("\n")}"
  end

  def wait_for_text(text, timeout: TIMEOUT)
    wait_for(timeout:) { |lines| lines.any? { |line| line.include?(text) } }
  end

  # Waits for the process to exit, returns its status
  def wait
    Timeout.timeout(TIMEOUT) { Process.wait2(@pid).last }.tap { @pump.join(TIMEOUT) }
  ensure
    close
  end

  # Closes the terminal without killing anything, like closing its window, and waits for the
  # process to exit, returns its status
  def hang_up
    @reader.close
    @writer.close
    wait
  end

  def close
    # The whole process group, a shell and what it started, which outlives the shell otherwise.
    # PTY.spawn makes the command the leader of its own session and group.
    Process.kill('KILL', -@pid)
  rescue Errno::ESRCH
    nil
  ensure
    @reader.close unless @reader.closed?
    @writer.close unless @writer.closed?
  end

  private

  def pump
    loop do
      chunk = @reader.readpartial(4096)
      @mutex.synchronize { feed(chunk) }
    end
  rescue Errno::EIO, IOError
    nil
  end

  def feed(chunk)
    @raw << chunk.dup.force_encoding(Encoding::UTF_8)
    @output << chunk.dup.force_encoding(Encoding::UTF_8)

    # A lone escape or an unfinished sequence at the end waits for the next chunk
    while (match = TOKEN.match(@raw))
      @raw = match.post_match
      apply(match)
    end
  end

  def apply(match)
    text = match[0]

    if match[2]
      control(match[1], match[2])
    elsif text == "\r"
      @column = 0
    elsif text == "\n"
      line_feed
    elsif text == "\b"
      @column = [@column - 1, 0].max
    elsif !text.start_with?("\e")
      put(text)
    end
  end

  def control(params, command)
    numbers = params.delete('?').split(';').map(&:to_i)

    case command
    when 'H', 'f' then move(numbers.fetch(0, 1) - 1, numbers.fetch(1, 1) - 1)
    when 'A' then move(@row - [numbers.fetch(0, 1), 1].max, @column)
    when 'B' then move(@row + [numbers.fetch(0, 1), 1].max, @column)
    when 'C' then move(@row, @column + [numbers.fetch(0, 1), 1].max)
    when 'D' then move(@row, @column - [numbers.fetch(0, 1), 1].max)
    when 'G' then move(@row, numbers.fetch(0, 1) - 1)
    when 'J' then clear_screen(numbers.fetch(0, 0))
    when 'K' then clear_line(numbers.fetch(0, 0))
    when '@' then insert_blanks([numbers.fetch(0, 1), 1].max)
    when 'P' then delete_chars([numbers.fetch(0, 1), 1].max)
    when 'n' then @writer.write("\e[#{@row + 1};#{@column + 1}R") if numbers == [6]
    end
  end

  # Readline inserts and deletes in the middle of a line by shifting the rest of it
  def insert_blanks(count)
    line = @grid[@row]
    line.insert(@column, *Array.new(count, ' '))
    line.pop(count)
  end

  def delete_chars(count)
    line = @grid[@row]
    line.slice!(@column, count)
    line.fill(' ', line.size...@columns)
  end

  def move(row, column)
    @row = row.clamp(0, @rows - 1)
    @column = column.clamp(0, @columns - 1)
  end

  def put(char)
    line_feed && (@column = 0) if @column >= @columns
    @grid[@row][@column] = char
    @column += 1
  end

  def line_feed
    if @row == @rows - 1
      @grid.shift
      @grid.push(Array.new(@columns, ' '))
    else
      @row += 1
    end
  end

  def clear_screen(mode)
    case mode
    when 0
      clear_line(0)
      ((@row + 1)...@rows).each { |row| @grid[row].fill(' ') }
    when 2, 3
      @grid.each { |row| row.fill(' ') }
    end
  end

  def clear_line(mode)
    case mode
    when 0 then @grid[@row].fill(' ', @column)
    when 1 then @grid[@row].fill(' ', 0, @column + 1)
    when 2 then @grid[@row].fill(' ')
    end
  end
end
