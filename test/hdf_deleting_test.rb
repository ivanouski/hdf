# frozen_string_literal: true

require_relative 'hdf_test_case'

class HdfDeletingTest < HdfTestCase
  def test_ctrl_d_deletes_every_occurrence_and_stays_open
    start('sbx run')

    delete
    assert_match(/ deleted sbx run codex ──\z/, @session.screen[DIVIDER_ROW])
    assert_equal ['> sbx run', 'sbx run claude (3)'], pane
    assert_equal HISTORY.sub("sbx run codex \n", ''), File.read(@history)

    delete
    assert_match(/ deleted sbx run claude ──\z/, @session.screen[DIVIDER_ROW])
    assert_equal ['> sbx run', 'no matches'], pane
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

    assert_equal '> claud', @session.screen.first
    assert_match(%r{ 3/14 ──\z}, @session.screen[DIVIDER_ROW])
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
end
