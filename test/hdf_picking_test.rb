# frozen_string_literal: true

require_relative 'hdf_test_case'

class HdfPickingTest < HdfTestCase
  def test_enter_prints_the_most_recent_match
    start('clau')

    assert_picks(:enter, 'Claude')
    assert_equal HISTORY, File.read(@history)
  end

  def test_tab_prints_the_selection_to_edit
    start('clau')

    assert_picks(:down, :tab, 'claude --resume', status: 4)
  end

  def test_tab_without_matches_prints_nothing
    start('nothing like it')

    assert_cancels(:tab)
  end

  # The only match, so Down stays on it
  def test_matches_the_words_of_the_query_in_any_order
    start('claude', 'run')

    assert_picks(:down, :enter, 'sbx run claude')
  end

  def test_down_selects_the_next_match
    start('clau')

    assert_picks(:down, :enter, 'claude --resume')
  end

  def test_up_and_control_keys_move_the_selection
    start('clau')

    assert_picks(:down, :down, :ctrl_n, :up, :ctrl_p, :down, :down, :enter, 'sbx build claude')
  end

  def test_selection_stops_at_the_last_match
    start('sbx run')

    assert_picks(:down, :down, :down, :enter, 'sbx run claude')
  end

  def test_scrolls_to_matches_below_the_pane
    start

    assert_picks(*[:down] * 12, :enter, 'tmux -CC')
  end

  def test_page_down_moves_the_selection_by_the_rows_of_the_list
    start

    assert_picks(:page_down, :down, :enter, 'sbx -V')
  end

  def test_page_up_moves_back_and_stops_at_the_first_match
    start

    assert_picks(:page_down, :page_down, :page_up, :page_up, :enter, 'gg')
  end

  def test_prints_the_command_exactly_as_stored
    File.write(@history, "printf 'a\tb'\nls my\\ \n")

    assert_equal "ls my\\ \n", picked_output(:enter)
    assert_equal "printf 'a\tb'\n", picked_output(:down, :enter)
  end

  def test_prints_the_most_recent_form_of_a_command
    File.write(@history, "ls -la\nls -la \n")

    assert_equal "ls -la \n", picked_output(:enter)

    File.write(@history, "ls -la \nls -la\n")

    assert_equal "ls -la\n", picked_output(:enter)
  end

  def test_prints_multi_line_commands_with_their_newlines
    File.write(@history, "#1700000000\nfor x in a b\ndo echo $x\ndone\n")

    assert_equal "for x in a b\ndo echo $x\ndone\n", picked_output(:enter)
  end

  def test_enter_without_matches_prints_nothing
    start('nothing like it')

    assert_cancels(:enter)
  end

  def test_escape_prints_nothing
    start('clau')

    assert_cancels(:escape)
    assert_equal HISTORY, File.read(@history)
  end

  def test_ctrl_c_prints_nothing
    start('clau')

    assert_cancels(:ctrl_c, status: 130)
  end

  def test_escape_prints_the_edited_query
    start('--print-query', 'sbx run')

    @session.press(:backspace, :backspace, :backspace, :backspace)

    assert_cancels(:escape, status: 3, output: 'sbx')
  end

  def test_enter_without_matches_prints_the_query
    start('--print-query', 'nothing like it')

    assert_cancels(:enter, status: 3, output: 'nothing like it')
  end

  def test_tab_without_matches_prints_the_query
    start('--print-query', 'nothing like it')

    assert_cancels(:tab, status: 3, output: 'nothing like it')
  end

  def test_keeps_trailing_whitespace_in_the_query
    output = File.join(@tmp_dir, 'output')
    start(command: ['sh', '-c', %("$0" "$@" > "#{output}"), HDF, '--print-query', 'sbx  '])

    @session.press(:escape)

    assert_equal 3, @session.wait.exitstatus
    assert_equal "sbx  \n", File.read(output)
  end

  def test_prints_an_empty_query
    File.write(@history, "ls\n")
    output = File.join(@tmp_dir, 'output')
    start(command: ['sh', '-c', %("$0" "$@" > "#{output}"), HDF, '--print-query', 'l'])

    @session.press(:backspace, :escape)

    assert_equal 3, @session.wait.exitstatus
    assert_equal "\n", File.read(output)
  end

  def test_ctrl_c_prints_no_query
    start('--print-query', 'sbx run')

    @session.press(:backspace)

    assert_cancels(:ctrl_c, status: 130)
  end

  def test_picking_prints_only_the_command
    start('--print-query', 'clau')

    assert_picks(:enter, 'Claude')
  end

  def test_prints_to_captured_stdout
    output = File.join(@tmp_dir, 'output')
    start(command: ['sh', '-c', %("$0" "$@" > "#{output}"), HDF, 'clau'])

    @session.press(:down, :enter)

    assert_equal 0, @session.wait.exitstatus
    assert_equal "claude --resume\n", File.read(output)
    assert_equal '', @session.screen.join.strip
  end
end
