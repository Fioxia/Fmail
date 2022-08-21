-- Postgres schema 
DROP TABLE IF EXISTS email;

CREATE TABLE email (
    id bigserial,
    sender varchar(255),
    receiver varchar(255),
    server varchar(100),
    data text
);