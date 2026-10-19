# frozen_string_literal: true

require_relative 'hdf_test_case'

class HdfTerminalTest < HdfTestCase
  def test_opens_on_the_cursor_line_and_clears_the_pane
    start('clau', start_row: 5)

    assert_equal '> clau', @session.screen[4].split.first(2).join(' ')

    @session.press(:enter)
    @session.wait

    assert_equal ['', '', '', '', 'Claude', ''], @session.screen.first(6)
  end

  def test_opens_below_text_on_the_cursor_line_and_returns_to_it
    start(command: ['sh', '-c', %(printf 'PROMPT$ '; exec "$0" "$@"), HDF, 'clau'])

    assert_equal 'PROMPT$', @session.screen[0]
    assert_equal ['> clau', 'Claude', 'claude --resume', 'sbx run claude (3)', 'sbx build claude'], pane

    @session.press(:enter)
    @session.wait

    assert_equal ['PROMPT$ Claude', ''], @session.screen.first(2)
  end

  def test_closes_the_pane_on_a_signal
    { 'TERM' => 143, 'INT' => 130, 'HUP' => 129 }.each do |signal, status|
      start('clau')

      Process.kill(signal, @session.pid)

      assert_equal status, @session.wait.exitstatus, signal
      assert_equal '', @session.screen.join("\n").strip, signal
    end
  end

  def test_exits_when_the_terminal_closes
    start('clau')

    assert_equal 129, @session.hang_up.exitstatus
  end

  def test_closes_the_pane_when_its_shell_is_gone
    start(command: ['sh', '-c', %q(sh -c '"$0" "$@"; :' "$0" "$@"; sleep 5), HDF, 'clau'])
    shell = Integer(`pgrep -P #{@session.pid} -x sh`)

    Process.kill('KILL', shell)

    @session.wait_for { |lines| lines.none? { |line| line.start_with?('> ') } }
  end

  def test_scrolls_the_screen_when_opened_at_the_bottom
    start('clau', start_row: 24)

    assert_equal ['> clau', 'Claude', 'claude --resume', 'sbx run claude (3)', 'sbx build claude'], pane
    assert_picks(:down, :enter, 'claude --resume')
  end

  def test_fits_a_short_terminal
    start('', rows: 4)

    # The prompt line and two commands
    assert_equal 3, pane.size
    assert_picks(:enter, 'gg')
  end
end
