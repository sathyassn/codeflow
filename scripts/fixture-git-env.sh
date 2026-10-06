#!/bin/sh
# Fixture Git and codeflow children must not launch detached maintenance.
export GIT_CONFIG_COUNT=2
export GIT_CONFIG_KEY_0=maintenance.auto
export GIT_CONFIG_VALUE_0=false
export GIT_CONFIG_KEY_1=gc.auto
export GIT_CONFIG_VALUE_1=0
export GIT_CONFIG_PARAMETERS=

