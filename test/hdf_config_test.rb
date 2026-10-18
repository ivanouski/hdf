# frozen_string_literal: true

require_relative 'hdf_test_case'

class HdfConfigTest < HdfTestCase
  def test_shows_the_selected_row_on_a_background_not_reversed
    start('cd')

    assert_color '48;5;238'
    refute_includes @session.output, "\e[7m"
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

  def test_init_config_leaves_no_half_written_config
    path = File.join(config_home, 'hdf', 'config.toml')
    _, stderr, status = Open3.capture3(
      { 'XDG_CONFIG_HOME' => config_home }, 'sh', '-c', %(trap '' XFSZ; ulimit -f 0; exec "$0" --init-config), HDF
    )

    assert_equal 2, status.exitstatus
    assert_match(/\Ahdf: cannot create #{Regexp.escape(path)}: File too large/, stderr)
    refute File.exist?(path)
  end

  def test_init_config_fails_on_a_closed_stdout
    stderr, status = run_hdf_into_a_closed_pipe('--init-config')

    assert_equal 2, status.exitstatus
    assert_equal "hdf: cannot print: Broken pipe (os error 32)\n", stderr
    assert File.exist?(File.join(config_home, 'hdf', 'config.toml'))
  end

  def test_starts_with_the_written_config
    run_hdf('--init-config')
    start('cd')

    assert_color '48;5;238'
  end
end
