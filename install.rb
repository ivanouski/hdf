#!/usr/bin/env ruby
# frozen_string_literal: true

require 'fileutils'

class Install
  TARGETS = {
    completions: '.completions',
    home: '',
  }.freeze

  def call
    TARGETS.each do |package, target|
      create_directory(target)
      stow(package.to_s, target)
    end

    run('cargo install --quiet --path .')
  end

  private

  def create_directory(directory)
    FileUtils.mkdir_p(File.expand_path(directory, Dir.home))
  end

  def stow(package, target)
    path = File.expand_path(target, Dir.home)

    run(%(stow --verbose #{package} -t "#{path}"))
  end

  def run(command)
    puts(command)
    abort("ERROR #{command} failed") unless system(command)
  end
end

$stdout.sync = true

Dir.chdir(__dir__)

Install.new.call

exit(0)
