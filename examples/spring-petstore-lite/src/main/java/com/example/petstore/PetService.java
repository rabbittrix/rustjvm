package com.example.petstore;

import org.springframework.stereotype.Service;

@Service
public class PetService {

    public String describe(String id) {
        return "Pet #" + id;
    }
}
