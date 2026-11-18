Feature: reading the configuration

  @id:config/rejects-missing
  Scenario: A directory with no config.json is refused
    Given the fixtures/empty-directory fixture
    When run-scanner runs on it

  @id:config/one-directory
  Scenario: A populated directory is accepted
    Given the fixtures/populated-directory fixture
    When run-scanner runs on it
