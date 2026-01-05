; CMake Manifest Tags (CMakeLists.txt)
;
; Captures for component extraction from CMake files.
; Used by TagCategory::Manifest for building Component nodes and DependsOn edges.

; ============================================================================
; Project Name (Component Name)
; ============================================================================

; project(my-project VERSION 1.0.0 LANGUAGES CXX)
; Captures the first argument as the project name
(normal_command
  (identifier) @_cmd
  (argument_list
    (argument
      (unquoted_argument) @manifest.component.name.cmake))
  (#eq? @_cmd "project"))

; ============================================================================
; CMake Minimum Version
; ============================================================================

; cmake_minimum_required(VERSION 3.20)
(normal_command
  (identifier) @_cmd
  (argument_list
    (argument) @_version_keyword
    (argument
      (unquoted_argument) @manifest.version.cmake.minimum))
  (#eq? @_cmd "cmake_minimum_required")
  (#eq? @_version_keyword "VERSION"))

; ============================================================================
; External Dependencies (find_package)
; ============================================================================

; find_package(Boost REQUIRED)
(normal_command
  (identifier) @_cmd
  (argument_list
    (argument
      (unquoted_argument) @manifest.dependency.cmake.external))
  (#eq? @_cmd "find_package"))

; ============================================================================
; Local Subdirectory Dependencies (for DependsOn edges)
; ============================================================================

; add_subdirectory(../shared shared_build)
; Captures the directory path (first argument)
(normal_command
  (identifier) @_cmd
  (argument_list
    (argument
      (unquoted_argument) @manifest.dependency.cmake.subdirectory))
  (#eq? @_cmd "add_subdirectory"))

; ============================================================================
; Executable Targets
; ============================================================================

; add_executable(my-app src/main.cpp)
(normal_command
  (identifier) @_cmd
  (argument_list
    (argument
      (unquoted_argument) @manifest.target.cmake.executable))
  (#eq? @_cmd "add_executable"))

; ============================================================================
; Library Targets
; ============================================================================

; add_library(mylib STATIC src/lib.cpp)
(normal_command
  (identifier) @_cmd
  (argument_list
    (argument
      (unquoted_argument) @manifest.target.cmake.library))
  (#eq? @_cmd "add_library"))

; ============================================================================
; Target Link Libraries (dependency tracking)
; ============================================================================

; target_link_libraries(my-app PRIVATE shared utils)
; Captures the target name (first argument)
(normal_command
  (identifier) @_cmd
  (argument_list
    (argument
      (unquoted_argument) @manifest.link.cmake.target))
  (#eq? @_cmd "target_link_libraries"))

; ============================================================================
; Include Directories (for discovering structure)
; ============================================================================

; include_directories(include ../common/include)
(normal_command
  (identifier) @_cmd
  (argument_list
    (argument
      (unquoted_argument) @manifest.include.cmake.directory))
  (#eq? @_cmd "include_directories"))

; ============================================================================
; FetchContent (modern CMake external deps)
; ============================================================================

; FetchContent_Declare(json URL https://...)
(normal_command
  (identifier) @_cmd
  (argument_list
    (argument
      (unquoted_argument) @manifest.dependency.cmake.fetch))
  (#eq? @_cmd "FetchContent_Declare"))
