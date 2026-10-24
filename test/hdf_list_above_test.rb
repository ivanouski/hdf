# frozen_string_literal: true

require_relative 'hdf_test_case'

class HdfListAboveTest < HdfTestCase
  def setup
    super
    write_config(%(list = "above"\n))
  end

  def test_lists_the_commands_up_from_the_prompt_line_on_the_last_row
    start('clau')

    assert_equal '> clau', @session.screen[11]
    assert_equal ['> clau', 'Claude', 'claude --resume', 'sbx run claude (3)', 'sbx build claude'], pane_above
    assert_equal [11, '> clau'.size], @session.cursor
  end

  def test_up_moves_to_older_commands
    start('clau')

    assert_picks(:up, :up, :down, :enter, 'claude --resume')
  end

  def test_keeps_the_prompt_line_on_the_cursor_line_when_opened_at_the_bottom
    start('clau', start_row: 24)

    assert_equal '> clau', @session.screen[23]
    assert_equal ['> clau', 'Claude', 'claude --resume', 'sbx run claude (3)', 'sbx build claude'], pane_above
  end

  def test_leaves_the_shell_prompt_on_the_first_row_when_closed
    start('--prompt', '$ ', 'clau', start_row: 5)

    assert_equal '$ clau', @session.screen[15]

    @session.press(:escape)
    @session.wait

    assert_equal ['$'], @session.screen.reject(&:empty?)
    assert_equal '$', @session.screen[4]
    assert_equal [4, 0], @session.cursor
  end

  def test_draws_the_border_on_the_first_row_of_the_pane
    write_config(%(list = "above"\nborder = true\n))
    start('clau')

    assert_equal '─' * 80, @session.screen[0]
    assert_equal ['> clau', 'Claude', 'claude --resume', 'sbx run claude (3)', 'sbx build claude'], pane_above
    assert_equal '> clau', @session.screen[11]
  end

  def test_fits_a_short_terminal
    start('', rows: 4)

    # The prompt line and two commands
    assert_equal ['>', 'gg (2)', 'Claude'], pane_above
    assert_picks(:up, :enter, 'Claude')
  end

  def test_up_in_bash_opens_hdf_and_goes_back_in_history
    File.write(@history, "echo first\necho picked\nls\n")
    write_config(%(list = "above"\n), home: File.join(@tmp_dir, '.config'))
    start_bash

    @session.press(:up)
    @session.wait_for { |lines| lines[11] == '$' && lines[10].match?(PANE_DIVIDER) }
    @session.press(:up, :enter)

    @session.wait_for { |lines| lines.first(3) == ['$ echo picked', 'picked', '$'] }
  end

  private

  # The prompt line under the divider, then the commands over it from the divider up
  def pane_above
    lines = @session.screen
    divider = lines.index { |line| line.match?(PANE_DIVIDER) }
    flunk("no pane over a prompt line:\n#{lines.join("\n")}") unless divider && lines[divider + 1]

    [lines[divider + 1], *lines[...divider].reverse.take_while { |line| !line.empty? }.map(&:strip)]
  end
end
