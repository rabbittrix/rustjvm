package com.example.di;

/**
 * A plain class — no stereotype annotation. It enters the container only
 * because AppConfig declares it via @Bean.
 */
public class PrefixService {

    public String prefix() {
        return "[rust] ";
    }
}
