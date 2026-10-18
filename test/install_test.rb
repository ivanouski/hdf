# frozen_string_literal: true

require_relative 'test_helper'

class InstallTest < Minitest::Test
  INSTALL = File.join(ROOT, 'install.rb')
  FAKE_BIN = File.join(ROOT, 'test', 'fake_bin')

  def setup
    @tmp_dir = File.realpath(Dir.mktmpdir)
    @home = File.join(@tmp_dir, 'home')
    @log = File.join(@tmp_dir, 'stow.jsonl')
    @cargo_log = File.join(@tmp_dir, 'cargo.jsonl')
    FileUtils.mkdir_p(@home)
  end

  def teardown
    FileUtils.remove_entry(@tmp_dir)
  end

  def test_creates_the_target_directories
    _, _, status = install

    assert status.success?
    assert File.directory?(File.join(@home, '.completions'))
  end

  def test_stows_the_completion_and_hdfrc
    install

    assert_equal [
      [File.realpath(ROOT), '--verbose', 'completions', '-t', File.join(@home, '.completions')],
      [File.realpath(ROOT), '--verbose', 'home', '-t', @home],
    ], stow_calls
  end

  def test_installs_the_crate_into_the_default_cargo_root
    install

    assert_equal [[File.realpath(ROOT), 'install', '--quiet', '--path', '.']], cargo_calls
  end

  def test_installs_the_crate_after_stowing
    install(env: { 'FAKE_STOW_FAIL' => 'completions' })

    assert_empty cargo_calls
  end

  def test_aborts_when_cargo_fails
    _, stderr, status = install(env: { 'FAKE_CARGO_FAIL' => '1' })

    refute status.success?
    assert_equal "ERROR cargo install --quiet --path . failed\n", stderr
  end

  def test_prints_each_command
    stdout, _, status = install

    assert status.success?
    assert_equal <<~OUTPUT, stdout
      stow --verbose completions -t "#{@home}/.completions"
      stow --verbose home -t "#{@home}"
      cargo install --quiet --path .
    OUTPUT
  end

  def test_shows_the_output_of_the_commands
    stdout, stderr, = install(env: { 'FAKE_STOW_OUTPUT' => 'LINK: .hdfrc => src/hdf/home/.hdfrc' })

    assert_includes stdout + stderr, 'LINK: .hdfrc => src/hdf/home/.hdfrc'
  end

  def test_stops_printing_at_a_failure
    stdout, = install(env: { 'FAKE_STOW_FAIL' => 'completions' })

    assert stdout.end_with?(%(stow --verbose completions -t "#{@home}/.completions"\n)), stdout
  end

  def test_runs_from_the_repo_root_wherever_started
    _, _, status = install(chdir: @tmp_dir)

    assert status.success?
    assert_equal [File.realpath(ROOT)], (stow_calls + cargo_calls).map(&:first).uniq
  end

  def test_keeps_existing_target_directories
    FileUtils.mkdir_p(File.join(@home, '.completions'))
    File.write(File.join(@home, '.completions', 'other'), '')

    _, _, status = install

    assert status.success?
    assert File.exist?(File.join(@home, '.completions', 'other'))
  end

  def test_aborts_when_stow_fails
    _, stderr, status = install(env: { 'FAKE_STOW_FAIL' => 'completions' })

    refute status.success?
    assert_equal %(ERROR stow --verbose completions -t "#{File.join(@home, '.completions')}" failed\n), stderr
    assert_equal 1, stow_calls.size
  end

  def test_links_with_the_real_stow
    skip 'stow is not installed' unless system('stow', '--version', out: File::NULL, err: File::NULL)

    fake_cargo_dir = File.join(@tmp_dir, 'fake_cargo')
    FileUtils.mkdir_p(fake_cargo_dir)
    File.symlink(File.join(FAKE_BIN, 'cargo'), File.join(fake_cargo_dir, 'cargo'))

    _, stderr, status = install(path: "#{fake_cargo_dir}:#{ENV.fetch('PATH')}")

    assert status.success?, stderr
    assert_equal File.join(ROOT, 'completions', '_hdf'), File.realpath(File.join(@home, '.completions', '_hdf'))
    assert_equal File.join(ROOT, 'home', '.hdfrc'), File.realpath(File.join(@home, '.hdfrc'))
  end

  private

  def install(path: "#{FAKE_BIN}:#{ENV.fetch('PATH')}", env: {}, chdir: ROOT)
    Open3.capture3(
      { 'PATH' => path, 'HOME' => @home, 'FAKE_STOW_LOG' => @log,
        'FAKE_CARGO_LOG' => @cargo_log, }.merge(env), INSTALL, chdir:
    )
  end

  def stow_calls
    calls(@log)
  end

  def cargo_calls
    calls(@cargo_log)
  end

  def calls(log)
    File.exist?(log) ? File.readlines(log).map { |line| JSON.parse(line) } : []
  end
end
