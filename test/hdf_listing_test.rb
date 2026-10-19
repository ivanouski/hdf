# frozen_string_literal: true

require_relative 'hdf_test_case'

class HdfListingTest < HdfTestCase
  def test_lists_matches_most_recent_first
    start('clau')

    assert_equal ['> clau', 'Claude', 'claude --resume', 'sbx run claude (3)', 'sbx build claude'], pane
  end

  def test_shows_the_match_count
    start('clau')

    assert_equal '> clau', @session.screen.first
    assert_match(%r{\A─+ 4/15 ──\z}, @session.screen[DIVIDER_ROW])
  end

  def test_lists_every_command_without_a_query
    start

    assert_equal 10, pane.size - 1
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
    @session.wait_for { |lines| lines.first == '>' }

    assert_equal 10, pane.size - 1
  end

  def test_edits_the_query_at_the_cursor
    start('sbx rn')
    @session.press(:left)
    @session.type('u')
    @session.wait_for_text('> sbx run')

    assert_equal ['> sbx run', 'sbx run codex', 'sbx run claude (3)'], pane
    @session.wait_for { @session.cursor == [0, '> sbx ru'.size] }

    @session.press(:home, :delete, :end)
    @session.type('!')

    @session.wait_for_text('> bx run!')
  end

  def test_scrolls_a_long_query_to_the_cursor
    query = 'docker compose -f deploy/compose.yml up --build'
    File.write(@history, "#{query} web\n")
    start(query, columns: 40)
    @session.type(' web')

    @session.wait_for { |lines| lines.first == "> …#{"#{query} web"[-36..]}" }
    @session.wait_for { @session.cursor == [0, 39] }
  end

  def test_shows_the_match_of_a_long_command_and_previews_it
    command = 'kubectl apply -f k8s/prod/ --context=prod-eu-west-1 --server-side ' \
              '--force-conflicts && ./deploy.sh notify'
    File.write(@history, "#{command}\nls\n")
    start('deploy')

    assert_equal "  …#{command[-77..]}", @session.screen[FIRST_ROW]

    divider = @session.screen.index('─' * 80)

    assert_equal command, @session.screen[(divider + 1)..].take_while { |line| !line.empty? }.join(' ')
  end

  def test_ignores_letters_typed_with_alt
    start('clau')

    @session.type("\ex")
    @session.type('d')
    @session.wait_for { |lines| lines.first == '> claud' }

    assert_equal ['> claud', 'Claude', 'claude --resume', 'sbx run claude (3)', 'sbx build claude'], pane
  end

  def test_says_so_without_matches
    start('nothing like it')

    assert_equal '> nothing like it', @session.screen.first
    assert_match(%r{ 0/15 ──\z}, @session.screen[DIVIDER_ROW])
    assert_equal ['> nothing like it', 'no matches'], pane
  end

  def test_matches_stars_literally
    File.write(@history, "claude\nls *.rb\n")
    start('*aude')

    assert_equal ['> *aude', 'no matches'], pane

    @session.press(:ctrl_u)
    @session.type('*.rb')
    @session.wait_for { |lines| lines.first == '> *.rb' }

    assert_equal ['> *.rb', 'ls *.rb'], pane
  end
end
