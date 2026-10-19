# frozen_string_literal: true

require_relative 'hdf_test_case'

class HdfTimestampsTest < HdfTestCase
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

    assert_equal '                    ls', @session.screen[FIRST_ROW + 1]
  end

  def test_takes_the_time_from_the_latest_timestamped_run
    File.write(@history, "pwd\n#1700000000\nls\nls\n")
    start

    assert_equal ['>', '2023-11-14 22:13  ls (2)', 'pwd'], pane
  end

  def test_matches_commands_not_times
    File.write(@history, "#1700000000\npwd\n")
    start('2023')

    assert_equal ['> 2023', 'no matches'], pane
  end
end
