# frozen_string_literal: true

require_relative 'test_helper'

class HdfCompletionTest < Minitest::Test
  COMPLETION = File.join(ROOT, 'completions', '_hdf')

  COMPLETE_LAST_WORD = <<~'BASH'
    source "$1"
    shift
    COMP_WORDS=("$@")
    COMP_CWORD=$(( $# - 1 ))
    _hdf
    printf '%s\n' "${COMPREPLY[@]}"
  BASH

  def test_dash_dash_offers_every_long_option
    assert_equal %w[--print-query --prompt --delete-last --init-config --help --version], complete('--')
  end

  def test_completes_a_partial_long_option
    assert_equal %w[--print-query --prompt], complete('--pr')
    assert_equal %w[--prompt], complete('--pro')
    assert_equal %w[--init-config], complete('--i')
    assert_equal %w[--delete-last], complete('--d')
  end

  def test_dash_offers_short_options
    assert_equal %w[-h -V], complete('-')
  end

  def test_query_words_are_not_completed
    assert_empty complete('')
    assert_empty complete('git', '')
    assert_empty complete('g')
  end

  def test_offers_print_query_and_prompt_after_the_query
    assert_equal %w[--print-query --prompt], complete('git', '--')
  end

  def test_options_that_exit_are_only_offered_first
    assert_empty complete('git', '-')
  end

  def test_print_query_and_prompt_are_not_offered_again
    assert_equal %w[--prompt], complete('--print-query', '--')
    assert_equal %w[--print-query], complete('--prompt', '$ ', '--')
  end

  def test_prompt_text_is_not_completed
    assert_empty complete('--prompt', '--')
    assert_empty complete('--prompt', '-')
  end

  def test_a_prompt_text_like_an_option_does_not_stop_completion
    assert_equal %w[--print-query], complete('--prompt', '--help', '--')
  end

  def test_nothing_follows_options_that_exit
    assert_empty complete('--help', '--')
    assert_empty complete('-h', '-')
    assert_empty complete('--version', '--')
    assert_empty complete('-V', '-')
    assert_empty complete('--init-config', '--')
    assert_empty complete('--delete-last', '--')
    assert_empty complete('--delete-last', '3', '--')
  end

  def test_nothing_follows_double_dash
    assert_empty complete('--', '--')
    assert_empty complete('--', '-')
  end

  private

  def complete(*words)
    stdout, stderr, status = Open3.capture3('bash', '-c', COMPLETE_LAST_WORD, 'bash', COMPLETION, 'hdf', *words)
    assert status.success?, stderr

    stdout.lines(chomp: true).reject(&:empty?)
  end
end
