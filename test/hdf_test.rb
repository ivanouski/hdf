# frozen_string_literal: true

require_relative 'test_helper'
require_relative 'terminal_session'

class HdfTest < Minitest::Test
  CRATE = File.join(ROOT, 'hdf')
  HDF_DIR = File.join(CRATE, 'target', 'debug')
  HDF = File.join(HDF_DIR, 'hdf')
  HDFRC = File.join(ROOT, 'home', '.hdfrc')

  # Bash keeps the trailing space of a completed word, so some commands repeat with and without one.
  # Quoted so editors that strip trailing whitespace leave them alone.
  HISTORY = [
    'cd src/sndbx/',
    'git fetch --all --prune',
    'sbx run claude ',
    'tmux -CC',
    'sbx -V',
    'sbx run claude ',
    'gg',
    'ga .',
    "git cm 'Align the recalculation status'",
    'sbx build claude ',
    'sbx run claude',
    'tc',
    'sbx run codex ',
    './install.rb ',
    'claude --resume',
    'git push ',
    'Claude',
    'gg',
  ].map { |line| "#{line}\n" }.join.freeze

  system('cargo', 'build', '--quiet', '--manifest-path', File.join(CRATE, 'Cargo.toml')) || abort('cargo build failed')

  def setup
    @tmp_dir = Dir.mktmpdir
    @history = File.join(@tmp_dir, 'bash_history')
    File.write(@history, HISTORY)
  end

  def teardown
    @session&.close
    FileUtils.remove_entry(@tmp_dir)
  end

  # Listing

  def test_lists_matches_most_recent_first
    start('clau')

    assert_equal ['> clau', 'Claude', 'claude --resume', 'sbx run claude (3)', 'sbx build claude'], pane
  end

  def test_shows_the_match_count
    start('clau')

    assert_match(%r{\A> clau +4/15\z}, @session.screen.first)
  end

  def test_lists_every_command_without_a_query
    start

    assert_equal 11, pane.size - 1 # all the 12 line pane holds
    assert_equal 'gg (2)', pane[1]
  end

  def test_matching_ignores_case
    start('CLAUDE --')

    assert_equal ['> CLAUDE --', 'claude --resume'], pane
  end

  def test_typing_narrows_the_list
    start('clau')
    @session.type('de --')
    @session.wait_for_text('> claude --')

    assert_equal ['> claude --', 'claude --resume'], pane
  end

  def test_backspace_and_ctrl_u_widen_the_list
    start('sbx r')
    @session.press(:backspace, :backspace)
    @session.wait_for_text('sbx -V')

    assert_equal ['> sbx', 'sbx run codex', 'sbx run claude (3)', 'sbx build claude', 'sbx -V'], pane

    @session.press(:ctrl_u)
    @session.wait_for { |lines| lines.first.start_with?('>  ') }

    assert_equal 11, pane.size - 1
  end

  def test_ignores_letters_typed_with_alt
    start('clau')

    @session.type("\eb")
    @session.type('d')
    @session.wait_for_text('> claud ')

    assert_equal ['> claud', 'Claude', 'claude --resume', 'sbx run claude (3)', 'sbx build claude'], pane
  end

  def test_shows_nothing_without_matches
    start('nothing like it')

    assert_match(%r{\A> nothing like it +0/15\z}, @session.screen.first)
    assert_equal ['> nothing like it'], pane
  end

  # Picking

  def test_enter_prints_the_most_recent_match
    start('clau')

    assert_picks(:enter, 'Claude')
    assert_equal HISTORY, File.read(@history)
  end

  def test_tab_prints_the_selection_like_enter
    start('clau')

    assert_picks(:down, :tab, 'claude --resume')
  end

  def test_tab_without_matches_prints_nothing
    start('nothing like it')

    assert_cancels(:tab)
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

  # Quitting with --print-query

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

  # Deleting

  def test_ctrl_d_deletes_every_occurrence_and_stays_open
    start('sbx run')

    delete
    assert_match(/deleted sbx run codex\z/, @session.screen.first)
    assert_equal ['> sbx run', 'sbx run claude (3)'], pane
    assert_equal HISTORY.sub("sbx run codex \n", ''), File.read(@history)

    delete
    assert_match(/deleted sbx run claude\z/, @session.screen.first)
    assert_equal ['> sbx run'], pane
    assert_equal HISTORY.lines.grep_v(/\Asbx run/).join, File.read(@history)
  end

  def test_selection_moves_to_the_next_command_after_deleting
    start('clau')

    @session.press(:down)
    delete

    assert_equal ['> clau', 'Claude', 'sbx run claude (3)', 'sbx build claude'], pane
    assert_picks(:enter, 'sbx run claude')
  end

  def test_selection_stays_on_the_last_command_after_deleting_it
    start('clau')

    @session.press(:down, :down, :down)
    delete

    assert_picks(:enter, 'sbx run claude')
  end

  def test_status_clears_on_the_next_key
    start('clau')

    delete
    @session.type('d')
    @session.wait_for_text('3/14')

    assert_match(%r{\A> claud +3/14\z}, @session.screen.first)
  end

  def test_ctrl_d_without_matches_changes_nothing
    start('nothing like it')

    @session.press(:ctrl_d)

    assert_cancels(:escape)
    assert_equal HISTORY, File.read(@history)
  end

  def test_deletions_stay_after_escape
    start('tmux')

    delete

    assert_cancels(:escape)
    assert_equal HISTORY.sub("tmux -CC\n", ''), File.read(@history)
  end

  # Special characters

  def test_matches_stars_literally
    File.write(@history, "claude\nls *.rb\n")
    start('*aude')

    assert_equal ['> *aude'], pane

    @session.press(:ctrl_u)
    @session.type('*.rb')
    @session.wait_for_text('> *.rb ')

    assert_equal ['> *.rb', 'ls *.rb'], pane
  end

  # History file

  def test_keeps_other_lines_byte_for_byte
    content = "caf\xC3\xA9\nbad \xFF byte\n\nsbx run claude\nlast line without newline".b
    File.binwrite(@history, content)
    start('sbx')

    delete

    assert_equal "caf\xC3\xA9\nbad \xFF byte\n\nlast line without newline".b, File.binread(@history)
  end

  def test_deletes_timestamps_with_their_commands
    File.write(@history, "#1700000000\nls\n#1700000001\nfor x in a b\ndo echo $x\ndone\n#1700000002\npwd\n")
    start('for x')

    assert_equal ['> for x', '2023-11-14 22:13  for x in a b do echo $x done'], pane

    delete

    assert_equal "#1700000000\nls\n#1700000002\npwd\n", File.read(@history)
  end

  def test_deletes_timestamps_that_start_mid_file
    File.write(@history, "ls\nsbx run claude\n#1700000000\npwd\n#1700000001\nsbx run claude\n")
    start

    assert_equal ['>', '2023-11-14 22:13  sbx run claude (2)', '2023-11-14 22:13  pwd', 'ls'], pane

    delete

    assert_equal "ls\n#1700000000\npwd\n", File.read(@history)
  end

  def test_keeps_lines_appended_while_open
    start('tmux')
    File.write(@history, "echo from another shell\n", mode: 'a')

    delete

    assert_equal "#{HISTORY.sub("tmux -CC\n", '')}echo from another shell\n", File.read(@history)
    assert_match(%r{ 0/15\z}, @session.screen.first.sub('deleted tmux -CC', '0/15'))
  end

  def test_refuses_to_write_a_file_rewritten_while_open
    start('')
    File.write(@history, "rewritten\n")

    @session.press(:ctrl_d)
    @session.wait_for_text('cannot delete')

    assert_includes @session.screen.first, 'cannot delete: history file was changed by another process'
    assert_equal ['>', 'rewritten'], pane
    assert_equal "rewritten\n", File.read(@history)
  end

  def test_keeps_permissions
    File.chmod(0o600, @history)
    start('tmux')

    delete

    assert_equal 0o600, File.stat(@history).mode & 0o777
  end

  def test_rewrites_the_target_of_a_symlink
    link = File.join(@tmp_dir, 'link')
    File.symlink(@history, link)
    start('tmux', history: link)

    delete

    assert File.symlink?(link)
    refute_includes File.read(@history), 'tmux'
    assert_equal %w[bash_history link], Dir.children(@tmp_dir).sort
  end

  def test_falls_back_to_bash_history_in_home
    File.write(File.join(@tmp_dir, '.bash_history'), "ls\npwd\n")
    start('pw', env: { 'HISTFILE' => nil, 'HOME' => @tmp_dir })

    delete

    assert_equal "ls\n", File.read(File.join(@tmp_dir, '.bash_history'))
  end

  def test_delete_last_deletes_the_last_command
    stdout, stderr, status = run_hdf('--delete-last')

    assert status.success?
    assert_empty stderr
    assert_equal "gg\n", stdout
    assert_equal HISTORY.lines[0...-1].join, File.read(@history)
  end

  def test_delete_last_deletes_the_last_n_commands_only
    stdout, _, status = run_hdf('--delete-last', '3')

    assert status.success?
    assert_equal "git push \nClaude\ngg\n", stdout
    assert_equal HISTORY.lines[0...-3].join, File.read(@history)
  end

  def test_delete_last_deletes_timestamps_with_their_commands
    File.write(@history, "#1\nls\n#2\nfor x\ndo\n")
    stdout, _, status = run_hdf('--delete-last')

    assert status.success?
    assert_equal "for x\ndo\n", stdout
    assert_equal "#1\nls\n", File.read(@history)
  end

  def test_delete_last_deletes_everything_when_asked_for_more
    run_hdf('--delete-last', '100')

    assert_empty File.read(@history)
  end

  def test_delete_last_rejects_an_invalid_count
    %w[0 x 2x -3 +2 --help].each do |count|
      _, stderr, status = run_hdf('--delete-last', count)

      assert_equal 2, status.exitstatus
      assert_equal "hdf: invalid count: #{count}\nTry 'hdf --help' for usage.\n", stderr
      assert_equal HISTORY, File.read(@history)
    end
  end

  def test_delete_last_takes_no_other_arguments
    [%w[--delete-last 3 foo], %w[docker --delete-last], %w[--print-query --delete-last]].each do |args|
      _, stderr, status = run_hdf(*args)

      assert_equal 2, status.exitstatus
      assert_equal "hdf: --delete-last takes no other arguments\nTry 'hdf --help' for usage.\n", stderr
      assert_equal HISTORY, File.read(@history)
    end
  end

  def test_delete_last_leaves_an_empty_history_alone
    File.write(@history, "\n")
    inode = File.stat(@history).ino
    stdout, _, status = run_hdf('--delete-last')

    assert status.success?
    assert_empty stdout
    assert_equal inode, File.stat(@history).ino
  end

  # Writing to a pipe without a reader fails, a closed stdout would not: Rust ignores EBADF on stdout
  def test_delete_last_succeeds_when_the_reader_is_gone
    reader, writer = IO.pipe
    reader.close
    stderr = File.join(@tmp_dir, 'stderr')
    pid = Process.spawn({ 'HISTFILE' => @history }, HDF, '--delete-last', '3', out: writer, err: stderr)
    writer.close
    _, status = Process.wait2(pid)

    assert status.success?
    assert_empty File.read(stderr)
    assert_equal HISTORY.lines[0...-3].join, File.read(@history)
  end

  # Timestamps

  def test_shows_when_each_command_last_ran
    File.write(@history, "#1700000000\nsbx run claude\n#1700003600\npwd\n#1700090000\nsbx run claude\n")
    start

    assert_equal ['>', '2023-11-15 23:13  sbx run claude (2)', '2023-11-14 23:13  pwd'], pane
  end

  def test_shows_times_in_the_local_time_zone
    File.write(@history, "#1700000000\npwd\n")
    start(env: { 'TZ' => 'Europe/Berlin' })

    assert_equal ['>', '2023-11-14 23:13  pwd'], pane
  end

  def test_leaves_the_time_blank_for_commands_from_before_timestamps
    File.write(@history, "ls\n#1700000000\npwd\n")
    start

    assert_equal '                    ls', @session.screen[2]
  end

  # The last ls comes from a shell without HISTTIMEFORMAT
  def test_takes_the_time_from_the_latest_timestamped_run
    File.write(@history, "pwd\n#1700000000\nls\nls\n")
    start

    assert_equal ['>', '2023-11-14 22:13  ls (2)', 'pwd'], pane
  end

  def test_matches_commands_not_times
    File.write(@history, "#1700000000\npwd\n")
    start('2023')

    assert_equal ['> 2023'], pane
  end

  # Terminal

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

  # crossterm keeps reading a closed terminal, so hdf has to exit for the SIGHUP on its own
  def test_exits_when_the_terminal_closes
    start('clau')

    assert_equal 129, @session.hang_up.exitstatus
  end

  # hdf under a shell that is not the session leader, so killing that shell sends hdf no SIGHUP
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

    assert_equal 4, pane.size
    assert_picks(:enter, 'gg')
  end

  # Bash setup

  def test_hdfrc_binds_page_up_and_page_down
    stdout, = Open3.capture3({ 'PATH' => "#{HDF_DIR}:#{ENV.fetch('PATH')}" }, *bash_with_hdfrc('bind -X'))

    # bind -X prints "\e[5~" "__hdf_search", with a colon after the key in older bash versions
    bindings = stdout.lines(chomp: true).map { |line| line.scan(/"([^"]+)"/).flatten }

    assert_equal [['\e[5~', '__hdf_search'], ['\e[6~', '__hdf_search']], bindings.sort
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

  def test_page_up_opens_hdf_with_the_typed_text_and_enter_puts_the_command_on_the_line
    File.write(@history, "echo first\necho picked\nls\n")
    start_bash

    @session.type('pick')
    @session.press(:page_up)
    @session.wait_for_text('> pick')

    assert_equal ['> pick', 'echo picked'], pane

    @session.press(:enter)
    @session.wait_for_text('$ echo picked')
    @session.press(:enter)

    @session.wait_for { |lines| lines.include?('picked') }
  end

  def test_tab_puts_the_command_on_the_line
    File.write(@history, "echo first\necho picked\n")
    start_bash

    @session.press(:page_up)
    @session.wait_for_text('> ')
    @session.press(:tab)

    @session.wait_for { |lines| lines.first == '$ echo picked' }
  end

  def test_page_up_on_an_empty_line_lists_every_command
    File.write(@history, "echo first\nls\n")
    start_bash

    @session.press(:page_up)
    @session.wait_for_text('> ')

    assert_equal ['>', 'ls', 'echo first'], pane
  end

  def test_up_and_down_browse_history_as_usual
    File.write(@history, "echo first\necho second\n")
    start_bash

    @session.press(:up)
    @session.wait_for { |lines| lines.first == '$ echo second' }
    @session.press(:up)
    @session.wait_for { |lines| lines.first == '$ echo first' }
    @session.press(:down)

    @session.wait_for { |lines| lines.first == '$ echo second' }
  end

  def test_page_down_opens_hdf_too
    File.write(@history, "echo first\nls\n")
    start_bash

    @session.type('echo')
    @session.press(:page_down)
    @session.wait_for_text('> echo')

    assert_equal ['> echo', 'echo first'], pane
  end

  def test_escape_keeps_the_typed_text
    start_bash

    @session.type('sbx')
    @session.press(:page_up)
    @session.wait_for_text('> sbx')
    @session.press(:escape)

    @session.wait_for { |lines| lines.first == '$ sbx' }
  end

  def test_escape_puts_the_edited_query_on_the_line
    start_bash

    @session.type('sbx run')
    @session.press(:page_up)
    @session.wait_for_text('> sbx run')
    @session.press(:backspace, :backspace, :backspace, :backspace)
    @session.wait_for { |lines| lines.first.start_with?('> sbx ') && !lines.first.include?('run') }
    @session.press(:escape)
    @session.wait_for { |lines| lines.first == '$ sbx' }
    @session.type('X')

    @session.wait_for { |lines| lines.first == '$ sbxX' }
  end

  def test_escape_without_edits_keeps_the_cursor
    start_bash

    @session.type("sbx run\e[D\e[D\e[D")
    @session.press(:page_up)
    @session.wait_for_text('> sbx run')
    @session.press(:escape)
    @session.wait_for { |lines| lines.first == '$ sbx run' }
    @session.type('X')

    @session.wait_for { |lines| lines.first == '$ sbx Xrun' }
  end

  def test_escape_keeps_trailing_spaces_of_the_query
    start_bash

    @session.type('sbx run')
    @session.press(:page_up)
    @session.wait_for_text('> sbx run')
    @session.press(:backspace, :backspace, :backspace)
    @session.wait_for { |lines| lines.first.start_with?('> sbx ') && !lines.first.include?('r') }
    @session.press(:escape)
    @session.wait_for { |lines| lines.first == '$ sbx' }
    @session.type('X')

    @session.wait_for { |lines| lines.first == '$ sbx X' }
  end

  def test_enter_without_matches_puts_the_query_on_the_line
    start_bash

    @session.type('sbx run')
    @session.press(:page_up)
    @session.wait_for_text('> sbx run')
    @session.type('zzz')
    @session.wait_for_text('> sbx runzzz')
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
    @session.press(:page_up)

    @session.wait_for_text('hdf: invalid option: --print-query')
    @session.wait_for { |lines| lines.any? { |line| line == '$ sbx run' } }
  end

  def test_ctrl_c_restores_the_typed_text
    start_bash

    @session.type('sbx run')
    @session.press(:page_up)
    @session.wait_for_text('> sbx run')
    @session.press(:backspace, :backspace, :backspace, :backspace)
    @session.wait_for { |lines| !lines.first.include?('run') }
    @session.press(:ctrl_c)

    @session.wait_for { |lines| lines.first == '$ sbx run' }
  end

  def test_an_error_leaves_the_typed_text
    start_bash
    # A directory, which hdf cannot read
    @session.type("HISTFILE=#{@tmp_dir}\r")
    @session.wait_for { |lines| lines[1] == '$' }

    @session.type('sbx run')
    @session.press(:page_up)

    @session.wait_for_text('hdf: cannot read')
    @session.wait_for { |lines| lines.last(24).any? { |line| line == '$ sbx run' } }
  end

  def test_finds_commands_from_the_current_shell
    File.write(@history, '')
    start_bash

    @session.type("echo fresh\r")
    @session.wait_for_text('fresh')
    @session.press(:page_up)
    @session.wait_for_text('> ')

    assert_equal ['>', 'echo fresh'], pane
  end

  def test_deleted_commands_leave_the_shell_history_at_the_next_prompt
    File.write(@history, "echo secret\necho keep\n")
    start_bash

    @session.press(:page_up)
    @session.wait_for_text('> ')
    @session.press(:down)
    delete
    @session.press(:escape)
    @session.wait_for { |lines| lines.first == '$' }
    @session.type("\rhistory\r")
    @session.wait_for_text('$ history')
    sleep(0.1)

    assert_equal ['$', '$ history', '    1  echo keep', '    2  history', '$'], @session.screen.first(5)
    assert_equal "echo keep\nhistory\n", File.read(@history)
  end

  def test_keeps_the_shell_history_intact
    File.write(@history, "echo first\necho second\n")
    start_bash

    @session.press(:page_up)
    @session.wait_for_text('> ')
    @session.press(:enter)
    @session.wait_for_text('$ echo second')
    @session.press(:ctrl_u)
    @session.type("history\r")
    @session.wait_for_text('$ history')
    sleep(0.1)

    assert_equal ['    1  echo first', '    2  echo second', '    3  history'], @session.screen.grep(/\A +\d/)
  end

  # Colors

  def test_shows_the_selected_row_on_a_background_not_reversed
    start('cd')

    assert_color '48;5;238'
    refute_includes @session.output, "\e[7m" # crossterm sends attributes on their own
  end

  def test_takes_colors_from_the_config
    write_config(<<~TOML)
      [colors]
      match = "#ff8800"
      selected = { bg = "21" }
    TOML
    start('cd')

    assert_color '38;2;255;136;0'
    assert_color '48;5;21'
    refute_color '48;5;238'
  end

  def test_falls_back_to_the_config_in_home
    write_config(%([colors]\nselected = { bg = "21" }\n), home: File.join(@tmp_dir, '.config'))
    start('cd', env: { 'XDG_CONFIG_HOME' => nil, 'HOME' => @tmp_dir })

    assert_color '48;5;21'
  end

  def test_fails_on_a_broken_config
    write_config(%([colors]\nmatch = "yelow"\n))
    _, stderr, status = run_hdf('ls')

    assert_equal 2, status.exitstatus
    assert_match(%r{\Ahdf: #{Regexp.escape(config_home)}/hdf/config.toml: TOML parse error at line 2}, stderr)
    assert_includes stderr, 'unknown color "yelow"'
  end

  def test_fails_on_an_unknown_key
    write_config(%([colors]\nmatched = "red"\n))
    _, stderr, status = run_hdf('ls')

    assert_equal 2, status.exitstatus
    assert_includes stderr, 'unknown field `matched`'
  end

  def test_init_config_writes_the_defaults_from_help
    path = File.join(config_home, 'hdf', 'config.toml')
    stdout, stderr, status = run_hdf('--init-config')
    help, = run_hdf('--help')
    defaults = help[/^    \[colors\]\n(?:    .*\n)+/].gsub(/^    /, '')

    assert status.success?, stderr
    assert_equal "#{path}\n", stdout
    assert_equal defaults, File.read(path)
  end

  def test_init_config_keeps_an_existing_config
    write_config('broken')
    path = File.join(config_home, 'hdf', 'config.toml')
    stdout, stderr, status = run_hdf('--init-config')

    assert_equal 2, status.exitstatus
    assert_empty stdout
    assert_equal "hdf: #{path} already exists\n", stderr
    assert_equal 'broken', File.read(path)
  end

  def test_init_config_falls_back_to_home
    stdout, _, status = run_hdf('--init-config', env: { 'XDG_CONFIG_HOME' => nil, 'HOME' => @tmp_dir })

    assert status.success?
    assert_equal "#{File.join(@tmp_dir, '.config', 'hdf', 'config.toml')}\n", stdout
  end

  def test_ignores_a_relative_config_home
    write_config(%([colors]\nselected = { bg = "21" }\n), home: File.join(@tmp_dir, '.config'))
    write_config('broken', home: File.join(@tmp_dir, 'relative'))
    start('cd', env: { 'XDG_CONFIG_HOME' => 'relative', 'HOME' => @tmp_dir },
                command: ['sh', '-c', %(cd "#{@tmp_dir}" && exec "$0" cd), HDF])

    assert_color '48;5;21'
  end

  def test_init_config_ignores_a_relative_config_home
    stdout, _, status = Open3.capture3({ 'HISTFILE' => @history, 'XDG_CONFIG_HOME' => 'relative', 'HOME' => @tmp_dir },
                                       HDF, '--init-config', chdir: @tmp_dir)

    assert status.success?
    assert_equal "#{File.join(@tmp_dir, '.config', 'hdf', 'config.toml')}\n", stdout
    refute File.exist?(File.join(@tmp_dir, 'relative'))
  end

  # A file size limit of 0 makes the write fail after the file was created, the signal it sends is ignored
  def test_init_config_leaves_no_half_written_config
    path = File.join(config_home, 'hdf', 'config.toml')
    _, stderr, status = Open3.capture3(
      { 'XDG_CONFIG_HOME' => config_home }, 'sh', '-c', %(trap '' XFSZ; ulimit -f 0; exec "$0" --init-config), HDF
    )

    assert_equal 2, status.exitstatus
    assert_match(/\Ahdf: cannot create #{Regexp.escape(path)}: File too large/, stderr)
    refute File.exist?(path)
  end

  def test_starts_with_the_written_config
    run_hdf('--init-config')
    start('cd')

    assert_color '48;5;238'
  end

  # Arguments and errors

  def test_prints_help
    stdout, stderr, status = run_hdf('--help')

    assert status.success?
    assert_empty stderr
    assert_match(/\AUsage: hdf \[OPTIONS\] \[QUERY\]$/, stdout)
  end

  def test_prints_the_version
    version = File.read(File.join(CRATE, 'Cargo.toml'))[/^version = "(.+)"$/, 1]

    %w[-V --version].each do |option|
      stdout, stderr, status = run_hdf(option)

      assert status.success?
      assert_empty stderr
      assert_equal "hdf #{version}\n", stdout
    end
  end

  def test_searches_for_version_options_after_double_dash
    File.write(@history, "sbx -V\nls\n")
    start('--', '-V')

    assert_equal ['> -V', 'sbx -V'], pane
  end

  def test_rejects_unknown_option
    _, stderr, status = run_hdf('-x')

    assert_equal 2, status.exitstatus
    assert_equal "hdf: invalid option: -x\nTry 'hdf --help' for usage.\n", stderr
  end

  def test_double_dash_starts_the_query
    File.write(@history, "ls\nls -la\n")
    start('--', '-la')

    assert_equal ['> -la', 'ls -la'], pane
  end

  def test_joins_words_into_one_query
    start('sbx', 'run')

    assert_equal ['> sbx run', 'sbx run codex', 'sbx run claude (3)'], pane
  end

  def test_fails_on_a_missing_history_file
    _, stderr, status = run_hdf('ls', env: { 'HISTFILE' => File.join(@tmp_dir, 'missing') })

    assert_equal 2, status.exitstatus
    assert_match(/\Ahdf: cannot read .*missing: No such file or directory/, stderr)
  end

  private

  def start(*query, command: [HDF, *query], history: @history, env: {}, **terminal)
    env = { 'HISTFILE' => history, 'TZ' => 'UTC', 'XDG_CONFIG_HOME' => config_home }.merge(env)
    @session = TerminalSession.new(command, env:, **terminal)
    wait_for_pane
  end

  # Runs the commands in an interactive bash, without user config, that sourced .hdfrc
  def bash_with_hdfrc(commands)
    ['bash', '--norc', '--noprofile', '-i', '-c', ". #{HDFRC}; #{commands}"]
  end

  # An interactive bash with the hdf setup, the dotfiles' history reload, and a $ prompt
  def start_bash(path: HDF_DIR)
    rc = File.join(@tmp_dir, 'bashrc')
    File.write(rc, <<~BASH)
      PS1='$ '
      HISTFILE=#{@history}
      PROMPT_COMMAND='history -a; history -c; history -r'
      . #{HDFRC}
    BASH
    env = {
      'PATH' => "#{path}:#{ENV.fetch('PATH')}", 'HOME' => @tmp_dir, 'XDG_CONFIG_HOME' => nil, 'INPUTRC' => File::NULL,
      'TZ' => 'UTC',
    }
    @session = TerminalSession.new(['bash', '--noprofile', '--rcfile', rc, '-i'], env:)
    @session.wait_for { |lines| lines.first == '$' }
  end

  def wait_for_pane
    @session.wait_for { |lines| lines.any? { |line| line.start_with?('> ') } }
    sleep(0.05) # let the first frame finish before reading it
  end

  # Lines of the pane: the prompt without the count or status, then the listed commands
  def pane
    lines = @session.screen
    top = lines.index { |line| line.start_with?('> ') }
    prompt = lines[top].sub(%r{ {2,}(\d+/\d+|deleted .*|cannot .*)\z}, '')

    [prompt, *lines[(top + 1)..].take_while { |line| !line.empty? }.map(&:strip)]
  end

  # Runs hdf with stdout captured, presses the keys, returns what it printed
  def picked_output(*keys)
    output = File.join(@tmp_dir, 'output')
    start(command: ['sh', '-c', %("$0" > "#{output}"), HDF])
    @session.press(*keys)
    @session.wait
    @session = nil

    File.read(output)
  end

  # Presses Ctrl-D and waits for the outcome
  def delete
    @session.press(:ctrl_d)
    @session.wait_for { |lines| lines.any? { |line| line.match?(/ (deleted|cannot) /) } }
    sleep(0.05)
  end

  # Presses the keys and checks hdf printed the command, the only thing left on the screen
  def assert_picks(*keys, command)
    @session.press(*keys)
    status = @session.wait

    assert_equal 0, status.exitstatus, @session.screen.join("\n")
    assert_equal command, @session.screen.join("\n").strip
  end

  # Presses the keys and checks hdf exited with the status, leaving only the output on the screen
  def assert_cancels(*keys, status: 1, output: '')
    @session.press(*keys)

    assert_equal status, @session.wait.exitstatus
    assert_equal output, @session.screen.join("\n").strip
  end

  def run_hdf(*, env: {})
    Open3.capture3({ 'HISTFILE' => @history, 'XDG_CONFIG_HOME' => config_home }.merge(env), HDF, *)
  end

  # Checks for a style in what hdf drew, crossterm sends a foreground and a background as one sequence
  def assert_color(code)
    assert_match(color_pattern(code), @session.output)
  end

  def refute_color(code)
    refute_match(color_pattern(code), @session.output)
  end

  def color_pattern(code)
    /\e\[(?:[\d;]*;)?#{Regexp.escape(code)}[;m]/
  end

  # Config of the tests, not of the user running them
  def config_home
    File.join(@tmp_dir, 'config')
  end

  def write_config(text, home: config_home)
    FileUtils.mkdir_p(File.join(home, 'hdf'))
    File.write(File.join(home, 'hdf', 'config.toml'), text)
  end
end
