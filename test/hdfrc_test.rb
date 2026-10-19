# frozen_string_literal: true

require_relative 'hdf_test_case'

class HdfrcTest < HdfTestCase
  def test_hdfrc_binds_up_and_down
    stdout, = Open3.capture3({ 'PATH' => "#{HDF_DIR}:#{ENV.fetch('PATH')}" }, *bash_with_hdfrc('bind -X; bind -s'))

    bindings = stdout.lines(chomp: true).map { |line| line.scan(/"([^"]*)"/).flatten }
    macro = '\e[9100~\e[9101~'

    assert_equal [['\eOA', macro], ['\eOB', macro], ['\e[9100~', '__hdf_search'], ['\e[A', macro], ['\e[B', macro]],
                 bindings.sort
  end

  def test_hdfrc_does_nothing_in_a_non_interactive_shell
    stdout, stderr, status = Open3.capture3({ 'PATH' => "#{HDF_DIR}:#{ENV.fetch('PATH')}" },
                                            'bash', '--norc', '--noprofile', '-c', ". #{HDFRC}; type __hdf_search")

    refute status.success?
    assert_empty stdout
    assert_equal ["bash: line 1: type: __hdf_search: not found\n"], [stderr]
  end

  def test_hdfrc_does_nothing_without_hdf
    stdout, stderr, status = Open3.capture3({ 'PATH' => '/usr/bin:/bin' },
                                            *bash_with_hdfrc('bind -X; type __hdf_search'))

    refute status.success?
    assert_empty stdout
    assert_includes stderr, '__hdf_search: not found'
  end

  def test_up_opens_hdf_with_the_typed_text_and_enter_runs_the_command
    File.write(@history, "echo first\necho picked\nls\n")
    start_bash

    @session.type('pick')
    @session.press(:up)
    wait_for_hdf_in_bash('pick')

    assert_equal ['$ pick', 'echo picked'], pane

    @session.press(:enter)

    @session.wait_for { |lines| lines.first(3) == ['$ echo picked', 'picked', '$'] }
  end

  def test_tab_puts_the_command_on_the_line_without_running_it
    File.write(@history, "echo first\necho picked\n")
    start_bash

    @session.press(:up)
    wait_for_hdf_in_bash
    @session.press(:tab)
    @session.wait_for { |lines| lines.first == '$ echo picked' }
    @session.type('X')

    @session.wait_for { |lines| lines.first == '$ echo pickedX' }
    assert_equal '', @session.screen[1]
  end

  def test_enter_runs_the_command_into_history
    File.write(@history, "echo first\necho second\n")
    start_bash

    @session.press(:up)
    wait_for_hdf_in_bash
    @session.press(:down, :enter)
    @session.wait_for { |lines| lines.include?('first') }
    @session.type("history\r")

    assert_history ['    1  echo first', '    2  echo second', '    3  echo first', '    4  history']
  end

  def test_up_on_an_empty_line_lists_every_command
    File.write(@history, "echo first\nls\n")
    start_bash

    @session.press(:up)
    wait_for_hdf_in_bash

    assert_equal ['$', 'ls', 'echo first'], pane
  end

  def test_shows_the_shell_prompt_in_its_colors
    File.write(@history, "echo first\necho picked\n")
    start_bash(ps1: '\[\e]0;title\a\e[35m\]box\[\e[0m\]\$ ')

    @session.type('pick')
    @session.press(:up)
    @session.wait_for { |lines| lines.first == 'box$ pick' && lines[DIVIDER_ROW].end_with?(' 1/2 ──') }

    assert_color '38;5;5'
    refute_includes @session.output.partition("\e[6n").last, 'title'
  end

  def test_lists_commands_under_the_query_after_a_long_prompt
    File.write(@history, "#1700000000\necho first\n#1700000000\necho picked\n")
    start_bash(ps1: '[mon@Aer ~/src/utils (main)]\$ ')

    @session.type('echo')
    @session.press(:up)
    @session.wait_for do |lines|
      lines.first == '[mon@Aer ~/src/utils (main)]$ echo' && lines[DIVIDER_ROW].end_with?(' 2/2 ──')
    end

    assert_equal "#{' ' * 12}2023-11-14 22:13  echo picked", @session.screen[FIRST_ROW]
  end

  def test_ctrl_p_and_ctrl_n_browse_history_as_usual
    File.write(@history, "echo first\necho second\n")
    start_bash

    @session.press(:ctrl_p)
    @session.wait_for { |lines| lines.first == '$ echo second' }
    @session.press(:ctrl_p)
    @session.wait_for { |lines| lines.first == '$ echo first' }
    @session.press(:ctrl_n)

    @session.wait_for { |lines| lines.first == '$ echo second' }
  end

  def test_down_opens_hdf_too
    File.write(@history, "echo first\nls\n")
    start_bash

    @session.type('echo')
    @session.press(:down)
    wait_for_hdf_in_bash('echo')

    assert_equal ['$ echo', 'echo first'], pane
  end

  def test_escape_keeps_the_typed_text
    start_bash

    @session.type('sbx')
    @session.press(:up)
    wait_for_hdf_in_bash('sbx')
    @session.press(:escape)

    @session.wait_for { |lines| lines.first == '$ sbx' }
  end

  def test_escape_puts_the_edited_query_on_the_line
    start_bash

    @session.type('sbx run')
    @session.press(:up)
    wait_for_hdf_in_bash('sbx run')
    @session.press(:backspace, :backspace, :backspace, :backspace)
    wait_for_hdf_in_bash('sbx')
    @session.press(:escape)
    @session.wait_for { |lines| lines.first == '$ sbx' }
    @session.type('X')

    @session.wait_for { |lines| lines.first == '$ sbxX' }
  end

  def test_escape_without_edits_keeps_the_cursor
    start_bash

    @session.type("sbx run\e[D\e[D\e[D")
    @session.press(:up)
    wait_for_hdf_in_bash('sbx run')
    @session.press(:escape)
    @session.wait_for { |lines| lines.first == '$ sbx run' }
    @session.type('X')

    @session.wait_for { |lines| lines.first == '$ sbx Xrun' }
  end

  def test_escape_keeps_trailing_spaces_of_the_query
    start_bash

    @session.type('sbx run')
    @session.press(:up)
    wait_for_hdf_in_bash('sbx run')
    @session.press(:backspace, :backspace, :backspace)
    wait_for_hdf_in_bash('sbx')
    @session.press(:escape)
    @session.wait_for { |lines| lines.first == '$ sbx' }
    @session.type('X')

    @session.wait_for { |lines| lines.first == '$ sbx X' }
  end

  def test_enter_without_matches_puts_the_query_on_the_line
    start_bash

    @session.type('sbx run')
    @session.press(:up)
    wait_for_hdf_in_bash('sbx run')
    @session.type('zzz')
    wait_for_hdf_in_bash('sbx runzzz')
    @session.press(:enter)

    @session.wait_for { |lines| lines.first == '$ sbx runzzz' }
  end

  def test_an_older_hdf_leaves_the_typed_text
    old_bin = File.join(@tmp_dir, 'old_bin')
    FileUtils.mkdir_p(old_bin)
    File.write(File.join(old_bin, 'hdf'), <<~SH)
      #!/bin/sh
      echo "hdf: invalid option: --print-query" >&2
      exit 1
    SH
    File.chmod(0o755, File.join(old_bin, 'hdf'))
    start_bash(path: old_bin)

    @session.type('sbx run')
    @session.press(:up)

    @session.wait_for_text('hdf: invalid option: --print-query')
    @session.wait_for { |lines| lines.any? { |line| line == '$ sbx run' } }
  end

  def test_ctrl_c_restores_the_typed_text
    start_bash

    @session.type('sbx run')
    @session.press(:up)
    wait_for_hdf_in_bash('sbx run')
    @session.press(:backspace, :backspace, :backspace, :backspace)
    @session.wait_for { |lines| !lines.first.include?('run') }
    @session.press(:ctrl_c)

    @session.wait_for { |lines| lines.first == '$ sbx run' }
  end

  def test_an_error_leaves_the_typed_text
    start_bash
    @session.type("HISTFILE=#{@tmp_dir}\r")
    @session.wait_for { |lines| lines[1] == '$' }

    @session.type('sbx run')
    @session.press(:up)

    @session.wait_for_text('hdf: cannot read')
    @session.wait_for { |lines| lines.last(24).any? { |line| line == '$ sbx run' } }
  end

  def test_finds_commands_from_the_current_shell
    File.write(@history, '')
    start_bash

    @session.type("echo fresh\r")
    @session.wait_for_text('fresh')
    @session.press(:up)
    wait_for_hdf_in_bash

    assert_equal ['$', 'echo fresh'], pane
  end

  def test_deleted_commands_leave_the_shell_history_at_the_next_prompt
    File.write(@history, "echo secret\necho keep\n")
    start_bash

    @session.press(:up)
    wait_for_hdf_in_bash
    @session.press(:down)
    delete
    @session.press(:escape)
    @session.wait_for { |lines| lines.first == '$' }
    @session.type("\rhistory\r")
    @session.wait_for { |lines| lines.first(5).last == '$' }

    assert_equal ['$', '$ history', '    1  echo keep', '    2  history', '$'], @session.screen.first(5)
    assert_equal "echo keep\nhistory\n", File.read(@history)
  end

  def test_keeps_the_shell_history_intact
    File.write(@history, "echo first\necho second\n")
    start_bash

    @session.press(:up)
    wait_for_hdf_in_bash
    @session.press(:tab)
    @session.wait_for_text('$ echo second')
    @session.press(:ctrl_u)
    @session.type("history\r")

    assert_history ['    1  echo first', '    2  echo second', '    3  history']
  end
end
