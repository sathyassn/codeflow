package main

import (
	"fmt"

	"github.com/codeflow/codeflow-cli/internal/config"
	"github.com/spf13/cobra"
)

// newConfigCmd creates the top-level "config" command with list, get, set subcommands.
func newConfigCmd() *cobra.Command {
	var global bool

	configCmd := &cobra.Command{
		Use:   "config",
		Short: "Manage CodeFlow configuration",
		Long:  "Read and modify CodeFlow configuration. Supports project-local (.codeflow/config/) and global (~/.config/codeflow/) settings.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	configCmd.PersistentFlags().BoolVar(&global, "global", false, "use global config (~/.config/codeflow/)")

	configCmd.AddCommand(newConfigListCmd(&global))
	configCmd.AddCommand(newConfigGetCmd(&global))
	configCmd.AddCommand(newConfigSetCmd(&global))

	return configCmd
}

// newConfigListCmd creates the "config list" subcommand.
func newConfigListCmd(global *bool) *cobra.Command {
	return &cobra.Command{
		Use:   "list",
		Short: "List all configuration keys and values",
		Long:  "Displays all configuration keys and values from project-local config. Use --global to show only global config.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runConfigList(cmd, *global)
		},
	}
}

// newConfigGetCmd creates the "config get" subcommand.
func newConfigGetCmd(global *bool) *cobra.Command {
	return &cobra.Command{
		Use:   "get <key>",
		Short: "Get a configuration value",
		Long:  "Retrieves a specific configuration value by dot-notation key (e.g., pathflow.rework_limits.max_rework_iterations).",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			return runConfigGet(cmd, args[0], *global)
		},
	}
}

// newConfigSetCmd creates the "config set" subcommand.
func newConfigSetCmd(global *bool) *cobra.Command {
	return &cobra.Command{
		Use:   "set <key> <value>",
		Short: "Set a configuration value",
		Long:  "Updates a configuration value with type validation. Use --global to write to global config (~/.config/codeflow/).",
		Args:  cobra.ExactArgs(2),
		RunE: func(cmd *cobra.Command, args []string) error {
			return runConfigSet(cmd, args[0], args[1], *global)
		},
	}
}

// configOpts builds the Options struct for config operations.
func configOpts() *config.Options {
	return &config.Options{
		ProjectDir: ".",
		GlobalDir:  defaultConfigDir(),
	}
}

// runConfigList implements the config list logic.
func runConfigList(cmd *cobra.Command, global bool) error {
	opts := configOpts()
	kv, err := config.List(opts, global)
	if err != nil {
		return &exitError{code: ExitConfigError, err: fmt.Errorf("listing config: %w", err)}
	}

	w := cmd.OutOrStdout()
	keys := config.SortedKeys(kv)
	for _, k := range keys {
		fmt.Fprintf(w, "%s = %v\n", k, kv[k])
	}

	return nil
}

// runConfigGet implements the config get logic.
func runConfigGet(cmd *cobra.Command, key string, global bool) error {
	opts := configOpts()
	val, err := config.Get(key, opts, global)
	if err != nil {
		return &exitError{code: ExitConfigError, err: fmt.Errorf("getting config key: %w", err)}
	}

	fmt.Fprintf(cmd.OutOrStdout(), "%v\n", val)
	return nil
}

// runConfigSet implements the config set logic.
func runConfigSet(cmd *cobra.Command, key, value string, global bool) error {
	opts := configOpts()
	if err := config.Set(key, value, opts, global); err != nil {
		return &exitError{code: ExitConfigError, err: fmt.Errorf("setting config key: %w", err)}
	}

	fmt.Fprintf(cmd.OutOrStdout(), "%s = %s\n", key, value)
	return nil
}
