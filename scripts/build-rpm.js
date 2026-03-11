#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

// Get arguments
const target = process.argv[2] || 'x86_64-unknown-linux-gnu';
const version = process.argv[3] || process.env.RELEASE_TAG || '0.1.0';
const outputDir = process.argv[4] || '.';

console.log(`Building RPM for target: ${target}`);
console.log(`Version: ${version}`);

// Clean version (remove v prefix if present)
const cleanVersion = version.replace(/^v/, '');

// Create rpm directory structure
const rpmDir = path.join(__dirname, '..', 'rpm');
const specDir = path.join(rpmDir, 'SPECS');
const buildDir = path.join(rpmDir, 'BUILD');
const rpmsDir = path.join(rpmDir, 'RPMS');
const sourcesDir = path.join(rpmDir, 'SOURCES');
const srpmsDir = path.join(rpmDir, 'SRPMS');

fs.mkdirSync(specDir, { recursive: true });
fs.mkdirSync(buildDir, { recursive: true });
fs.mkdirSync(rpmsDir, { recursive: true });
fs.mkdirSync(sourcesDir, { recursive: true });
fs.mkdirSync(srpmsDir, { recursive: true });

// Create spec file
const projectRoot = path.join(__dirname, '..');
// Use POSIX style path for Linux compatibility
const binaryPath = path.join(projectRoot, 'target', target, 'release', 'openixcli').replace(/\\/g, '/');

const specContent = `Name: openixcli
Version: ${cleanVersion}
Release: 1
Summary: A command-line firmware flashing tool for Allwinner chips
License: MIT
URL: https://github.com/YuzukiTsuru/OpenixSuit-cli

%description
OpenixCLI is a powerful and user-friendly CLI tool designed for flashing firmware to devices powered by Allwinner SoCs.

%files
%defattr(-,root,root,-)
/usr/bin/openixcli

%install
mkdir -p %{buildroot}/usr/bin
cp ${binaryPath} %{buildroot}/usr/bin/

%build

%prep
`;

const specFile = path.join(specDir, 'openixcli.spec');
fs.writeFileSync(specFile, specContent);
console.log(`Created spec file: ${specFile}`);

// Build RPM
try {
  console.log('Building RPM...');
  execSync(`rpmbuild --define "_topdir ${rpmDir}" --target x86_64 -bb ${specFile}`, {
    stdio: 'inherit',
    cwd: path.join(__dirname, '..')
  });
  
  // Find the built RPM
  const archDir = path.join(rpmsDir, 'x86_64');
  let rpmFiles = [];
  
  // Check in x86_64 subdirectory first
  if (fs.existsSync(archDir)) {
    rpmFiles = fs.readdirSync(archDir).filter(file => file.endsWith('.rpm'));
  }
  
  // If no files found, check root rpms directory
  if (rpmFiles.length === 0) {
    rpmFiles = fs.readdirSync(rpmsDir).filter(file => file.endsWith('.rpm'));
  }
  
  if (rpmFiles.length > 0) {
    const rpmFile = rpmFiles.length > 0 && fs.existsSync(archDir) ? 
      path.join(archDir, rpmFiles[0]) : 
      path.join(rpmsDir, rpmFiles[0]);
    const outputFile = path.join(outputDir, `openixcli-${target}.rpm`);
    
    fs.copyFileSync(rpmFile, outputFile);
    console.log(`RPM built successfully: ${outputFile}`);
  } else {
    console.error('No RPM file found after build');
    // List directory contents for debugging
    console.error('Contents of rpmsDir:', fs.readdirSync(rpmsDir));
    if (fs.existsSync(archDir)) {
      console.error('Contents of archDir:', fs.readdirSync(archDir));
    }
    process.exit(1);
  }
} catch (error) {
  console.error('Error building RPM:', error.message);
  process.exit(1);
}
