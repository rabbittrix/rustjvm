#!/bin/bash
# scripts/build-jar.sh — build and verify the RustJVM Spring API JAR.

set -e
cd "$(dirname "$0")/.."

echo "Building RustJVM Spring API JAR..."

cd rustjvm-spring-api

mvn clean compile
mvn test
mvn package

ls -la target/rustjvm-spring-api-*.jar

echo "JAR built successfully: rustjvm-spring-api/target/rustjvm-spring-api-0.1.0-alpha.jar"
