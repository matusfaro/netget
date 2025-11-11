-- MySQL Tool Call Test Schema
-- This file demonstrates that the LLM can read a SQL schema file and use it to respond to queries.
-- Assume this schema has been applied to the MySQL database.

CREATE TABLE users
(
    id       INT PRIMARY KEY,
    username VARCHAR(50)  NOT NULL,
    email    VARCHAR(100) NOT NULL,
    age      INT
);

-- Test data: 7 records
INSERT INTO users (id, username, email, age)
VALUES (1, 'alice', 'alice@example.com', 30),
       (2, 'bob', 'bob@example.com', 25),
       (3, 'charlie', 'charlie@example.com', 35),
       (4, 'diana', 'diana@example.com', 28),
       (5, 'eve', 'eve@example.com', 32),
       (6, 'frank', 'frank@example.com', 45),
       (7, 'grace', 'grace@example.com', 29);
