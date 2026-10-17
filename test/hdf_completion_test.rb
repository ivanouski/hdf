require_relative 'test_helper'

class HdfCompletionTest < Minitest::Test
  COMPLETION = File.join(ROOT, 'completions', '_hdf')

  # Sources the completion, completes the last word and prints the candidates
  SCRIPT = <<~'BASH'.freeze
    source "$1"
    shift
    COMP_WORDS=("$@")
    COMP_CWORD=$(( $# - 1 ))
    _hdf
    printf '%s\n' "${COMPREPLY[@]}"
  BASH

  def test_dash_dash_offers_every_long_option
    assert_equal %w[--print-query --init-config --help --version], complete('--')
  end

  def test_completes_a_partial_long_option
    assert_equal %w[--print-query], complete('--pr')
    assert_equal %w[--init-config], complete('--i')
  end

  def test_dash_offers_short_options
    assert_equal %w[-h -V], complete('-')
  end

  def test_query_words_are_not_completed
    assert_empty complete('')
    assert_empty complete('git', '')
    assert_empty complete('g')
  end

  def test_offers_print_query_after_the_query
    assert_equal %w[--print-query], complete('git', '--')
  end

  def test_options_that_exit_are_only_offered_first
    assert_empty complete('git', '-')
  end

  def test_print_query_is_not_offered_again
    assert_empty complete('--print-query', '--')
  end

  def test_nothing_follows_options_that_exit
    assert_empty complete('--help', '--')
    assert_empty complete('-h', '-')
    assert_empty complete('--version', '--')
    assert_empty complete('-V', '-')
    assert_empty complete('--init-config', '--')
  end

  def test_nothing_follows_double_dash
    assert_empty complete('--', '--')
    assert_empty complete('--', '-')
  end

  private

  def complete(*words)
    stdout, stderr, status = Open3.capture3('bash', '-c', SCRIPT, 'bash', COMPLETION, 'hdf', *words)
    assert status.success?, stderr

    stdout.lines(chomp: true).reject(&:empty?)
  end
end
