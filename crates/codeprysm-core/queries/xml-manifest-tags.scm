; XML Manifest Tags (.csproj, .vbproj, .fsproj)
;
; Captures for component extraction from MSBuild project files.
; Used by TagCategory::Manifest for building Component nodes and DependsOn edges.

; ============================================================================
; Assembly/Project Name (Component Name)
; ============================================================================

; <AssemblyName>MyProject</AssemblyName>
(element
  (STag
    (Name) @_tag)
  (content
    (CharData) @manifest.component.name.dotnet)
  (#eq? @_tag "AssemblyName"))

; <RootNamespace>MyOrg.MyProject</RootNamespace> (fallback for name)
(element
  (STag
    (Name) @_tag)
  (content
    (CharData) @manifest.component.namespace.dotnet)
  (#eq? @_tag "RootNamespace"))

; ============================================================================
; Project Version
; ============================================================================

; <Version>1.0.0</Version>
(element
  (STag
    (Name) @_tag)
  (content
    (CharData) @manifest.component.version.dotnet)
  (#eq? @_tag "Version"))

; <PackageVersion>1.0.0</PackageVersion>
(element
  (STag
    (Name) @_tag)
  (content
    (CharData) @manifest.component.packageversion.dotnet)
  (#eq? @_tag "PackageVersion"))

; ============================================================================
; Target Framework (useful metadata)
; ============================================================================

; <TargetFramework>net8.0</TargetFramework>
(element
  (STag
    (Name) @_tag)
  (content
    (CharData) @manifest.framework.dotnet)
  (#eq? @_tag "TargetFramework"))

; ============================================================================
; Package References (NuGet dependencies)
; ============================================================================

; <PackageReference Include="Newtonsoft.Json" Version="13.0.3" />
(element
  (EmptyElemTag
    (Name) @_tag
    (Attribute
      (Name) @_attr_include
      (AttValue) @manifest.dependency.nuget.name)
    (Attribute
      (Name) @_attr_version
      (AttValue) @manifest.dependency.nuget.version))
  (#eq? @_tag "PackageReference")
  (#eq? @_attr_include "Include")
  (#eq? @_attr_version "Version"))

; Package reference without version (for centralized package management)
(element
  (EmptyElemTag
    (Name) @_tag
    (Attribute
      (Name) @_attr_include
      (AttValue) @manifest.dependency.nuget.central.name))
  (#eq? @_tag "PackageReference")
  (#eq? @_attr_include "Include"))

; ============================================================================
; Project References (Local dependencies - for DependsOn edges)
; ============================================================================

; <ProjectReference Include="..\Shared\MyOrg.Shared.csproj" />
(element
  (EmptyElemTag
    (Name) @_tag
    (Attribute
      (Name) @_attr_include
      (AttValue) @manifest.dependency.projectref.dotnet))
  (#eq? @_tag "ProjectReference")
  (#eq? @_attr_include "Include"))

; ============================================================================
; Framework References
; ============================================================================

; <FrameworkReference Include="Microsoft.AspNetCore.App" />
(element
  (EmptyElemTag
    (Name) @_tag
    (Attribute
      (Name) @_attr_include
      (AttValue) @manifest.dependency.framework.dotnet))
  (#eq? @_tag "FrameworkReference")
  (#eq? @_attr_include "Include"))

; ============================================================================
; IsPackable detection (for publishability)
; ============================================================================

; <IsPackable>true</IsPackable>
(element
  (STag
    (Name) @_tag)
  (content
    (CharData) @manifest.packable.dotnet)
  (#eq? @_tag "IsPackable"))
