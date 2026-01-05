module github.com/myorg/cmd

go 1.21

require (
    github.com/spf13/cobra v1.7.0
    github.com/myorg/shared v0.0.0
    github.com/myorg/api v0.0.0
)

replace github.com/myorg/shared => ../shared

replace github.com/myorg/api => ../api
