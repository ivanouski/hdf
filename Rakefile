# frozen_string_literal: true

require 'rake/testtask'

Rake::TestTask.new do |task|
  task.pattern = 'test/**/*_test.rb'
end

desc 'Run the unit tests and clippy'
task :cargo do
  sh 'cargo test --quiet'
  sh 'cargo clippy --quiet --all-targets -- -D warnings'
end

desc 'Run rubocop'
task :rubocop do
  sh 'rubocop'
end

task default: %i[rubocop cargo test]
