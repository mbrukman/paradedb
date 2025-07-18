DROP TABLE IF EXISTS users;
DROP TABLE IF EXISTS products;
DROP TABLE IF EXISTS orders;

CREATE TABLE users
(
    id    serial8 not null primary key,
    name  text,
    color varchar,
    age   varchar
);

CREATE TABLE products
(
    id    serial8 not null primary key,
    name  text,
    color varchar,
    age   varchar
);

CREATE TABLE orders
(
    id    serial8 not null primary key,
    name  text,
    color varchar,
    age   varchar
);


CREATE INDEX idxusers ON users USING bm25 (id, name, color, age)
    WITH (
    key_field = 'id',
    text_fields = '
            {
                "name": { "tokenizer": { "type": "keyword" } },
                "color": { "tokenizer": { "type": "keyword" } },
                "age": { "tokenizer": { "type": "keyword" } }
            }'
    );
CREATE INDEX idxproducts ON products USING bm25 (id, name, color, age)
    WITH (
    key_field = 'id',
    text_fields = '
            {
                "name": { "tokenizer": { "type": "keyword" } },
                "color": { "tokenizer": { "type": "keyword" } },
                "age": { "tokenizer": { "type": "keyword" } }
            }'
    );
CREATE INDEX idxorders ON orders USING bm25 (id, name, color, age)
    WITH (
    key_field = 'id',
    text_fields = '
            {
                "name": { "tokenizer": { "type": "keyword" } },
                "color": { "tokenizer": { "type": "keyword" } },
                "age": { "tokenizer": { "type": "keyword" } }
            }'
    );

CREATE INDEX idxusers_name ON users (name);
CREATE INDEX idxusers_color ON users (color);
CREATE INDEX idxusers_age ON users (age);
CREATE INDEX idxproducts_name ON products (name);
CREATE INDEX idxproducts_color ON products (color);
CREATE INDEX idxproducts_age ON products (age);
CREATE INDEX idxorders_name ON orders (name);
CREATE INDEX idxorders_color ON orders (color);
CREATE INDEX idxorders_age ON orders (age);

INSERT INTO users
VALUES (1, 'bob', 'blue', '20');
INSERT INTO users
VALUES (2, 'anchovy', 'purple', '78');
INSERT INTO users
VALUES (3, 'sally', 'orange', '21');
INSERT INTO users
VALUES (4, 'alice', 'green', '40');
INSERT INTO users
VALUES (5, 'brandy', 'purple', '79');
INSERT INTO users
VALUES (6, 'anchovy', 'green', '69');
INSERT INTO users
VALUES (7, 'sally', 'green', '42');
INSERT INTO users
VALUES (8, 'bob', 'pink', '7');
INSERT INTO users
VALUES (9, 'cloe', 'green', '49');
INSERT INTO users
VALUES (10, 'brisket', 'purple', '65');
INSERT INTO users
VALUES (11, 'alice', 'pink', '39');

INSERT INTO products
VALUES (1, 'bob', 'blue', '20');
INSERT INTO products
VALUES (2, 'bob', 'pink', '20');
INSERT INTO products
VALUES (3, 'brandy', 'purple', '32');
INSERT INTO products
VALUES (4, 'alice', 'red', '46');
INSERT INTO products
VALUES (5, 'brandy', 'pink', '41');
INSERT INTO products
VALUES (6, 'brisket', 'yellow', '22');
INSERT INTO products
VALUES (7, 'alice', 'yellow', '6');
INSERT INTO products
VALUES (8, 'sally', 'yellow', '48');
INSERT INTO products
VALUES (9, 'brandy', 'purple', '69');
INSERT INTO products
VALUES (10, 'brandy', 'green', '21');
INSERT INTO products
VALUES (11, 'sally', 'yellow', '88');

INSERT INTO orders
VALUES (1, 'bob', 'blue', '20');
INSERT INTO orders
VALUES (2, 'brisket', 'green', '28');
INSERT INTO orders
VALUES (3, 'alice', 'yellow', '13');
INSERT INTO orders
VALUES (4, 'alice', 'purple', '44');
INSERT INTO orders
VALUES (5, 'brandy', 'green', '33');
INSERT INTO orders
VALUES (6, 'brisket', 'red', '58');
INSERT INTO orders
VALUES (7, 'cloe', 'purple', '34');
INSERT INTO orders
VALUES (8, 'brandy', 'red', '13');
INSERT INTO orders
VALUES (9, 'bob', 'green', '75');
INSERT INTO orders
VALUES (10, 'cloe', 'red', '53');
INSERT INTO orders
VALUES (11, 'brandy', 'green', '92');


ANALYZE;

--
-- each pair of queries should produce the same number of results
--


---- idx=50 ----
---- connector=AND ----
SELECT (SELECT COUNT(*)
        FROM users
                 LEFT JOIN products ON users.color = products.color
        WHERE (users.name = 'bob') AND ((users.color = 'blue') AND (users.name = 'bob')) AND (products.name = 'bob')
           OR (NOT (products.age = '20')) AND (users.name = 'bob')
           OR ((users.color = 'blue') AND (users.name = 'bob')) AND (products.color = 'blue')
           OR (NOT (products.name = 'bob'))),
       (SELECT COUNT(*)
        FROM users
                 LEFT JOIN products ON users.color = products.color
        WHERE (users.name @@@ 'bob') AND ((users.color @@@ 'blue') AND (users.name @@@ 'bob')) AND
              (products.name @@@ 'bob')
           OR (NOT (products.age @@@ '20')) AND (users.name @@@ 'bob')
           OR ((users.color @@@ 'blue') AND (users.name @@@ 'bob')) AND (products.color @@@ 'blue')
           OR (NOT (products.name @@@ 'bob')));

---- idx=4 ----
---- connector=AND NOT ----
SELECT (SELECT COUNT(*)
        FROM users
                 JOIN orders ON users.color = orders.color
        WHERE (users.name = 'bob') AND ((users.color = 'blue') OR (NOT (users.name = 'bob'))) AND
              NOT ((orders.name = 'bob') AND (orders.age = '20'))
           OR (orders.age = '20') AND NOT (users.name = 'bob')
           OR ((users.color = 'blue') OR (NOT (users.name = 'bob'))) AND
              NOT ((orders.name = 'bob') OR (orders.age = '20')) AND (orders.name = 'bob')),
       (SELECT COUNT(*)
        FROM users
                 JOIN orders ON users.color = orders.color
        WHERE (users.name @@@ 'bob') AND ((users.color @@@ 'blue') OR (NOT (users.name @@@ 'bob'))) AND
              NOT ((orders.name @@@ 'bob') AND (orders.age @@@ '20'))
           OR (orders.age @@@ '20') AND NOT (users.name @@@ 'bob')
           OR ((users.color @@@ 'blue') OR (NOT (users.name @@@ 'bob'))) AND
              NOT ((orders.name @@@ 'bob') OR (orders.age @@@ '20')) AND (orders.name @@@ 'bob'));

---- idx=37 ----
---- connector=AND NOT ----
SELECT (SELECT COUNT(*)
        FROM users
                 JOIN products ON users.name = products.name
        WHERE (users.color = 'blue') AND ((users.name = 'bob') OR (NOT (users.color = 'blue'))) AND
              NOT (products.color = 'blue')
           OR ((products.color = 'blue') AND (products.color = 'blue')) AND NOT (users.color = 'blue')
           OR ((users.name = 'bob') OR (NOT (users.color = 'blue'))) AND NOT (products.color = 'blue') AND
              ((products.color = 'blue') OR (products.color = 'blue'))),
       (SELECT COUNT(*)
        FROM users
                 JOIN products ON users.name = products.name
        WHERE (users.color @@@ 'blue') AND ((users.name @@@ 'bob') OR (NOT (users.color @@@ 'blue'))) AND
              NOT (products.color @@@ 'blue')
           OR ((products.color @@@ 'blue') AND (products.color @@@ 'blue')) AND NOT (users.color @@@ 'blue')
           OR ((users.name @@@ 'bob') OR (NOT (users.color @@@ 'blue'))) AND NOT (products.color @@@ 'blue') AND
              ((products.color @@@ 'blue') OR (products.color @@@ 'blue')));

---- idx=46 ----
---- connector=AND NOT ----
SELECT (SELECT COUNT(*)
        FROM users
                 LEFT JOIN products ON users.name = products.name
        WHERE (users.color = 'blue') AND ((users.age = '20') OR (NOT (users.color = 'blue'))) AND
              NOT (products.color = 'blue')
           OR ((products.age = '20') OR (products.age = '20')) AND NOT (users.color = 'blue')
           OR ((users.age = '20') OR (NOT (users.color = 'blue'))) AND NOT (products.age = '20') AND
              (NOT (NOT (products.name = 'bob')))),
       (SELECT COUNT(*)
        FROM users
                 LEFT JOIN products ON users.name = products.name
        WHERE (users.color @@@ 'blue') AND ((users.age @@@ '20') OR (NOT (users.color @@@ 'blue'))) AND
              NOT (products.color @@@ 'blue')
           OR ((products.age @@@ '20') OR (products.age @@@ '20')) AND NOT (users.color @@@ 'blue')
           OR ((users.age @@@ '20') OR (NOT (users.color @@@ 'blue'))) AND NOT (products.age @@@ '20') AND
              (NOT (NOT (products.name @@@ 'bob'))));

---- idx=55 ----
---- connector=AND NOT ----
SELECT (SELECT COUNT(*)
        FROM users
                 RIGHT JOIN products ON users.name = products.name
        WHERE (users.color = 'blue') AND ((NOT (users.color = 'blue')) OR (users.color = 'blue')) AND
              NOT (products.age = '20')
           OR ((products.name = 'bob') OR (products.age = '20')) AND NOT (users.color = 'blue')
           OR ((NOT (users.color = 'blue')) OR (users.color = 'blue')) AND NOT (products.age = '20') AND
              ((products.color = 'blue') AND (products.name = 'bob'))),
       (SELECT COUNT(*)
        FROM users
                 RIGHT JOIN products ON users.name = products.name
        WHERE (users.color @@@ 'blue') AND ((NOT (users.color @@@ 'blue')) OR (users.color @@@ 'blue')) AND
              NOT (products.age @@@ '20')
           OR ((products.name @@@ 'bob') OR (products.age @@@ '20')) AND NOT (users.color @@@ 'blue')
           OR ((NOT (users.color @@@ 'blue')) OR (users.color @@@ 'blue')) AND NOT (products.age @@@ '20') AND
              ((products.color @@@ 'blue') AND (products.name @@@ 'bob')));

---- idx=83 ----
---- connector=AND NOT ----
SELECT (SELECT COUNT(*)
        FROM orders
                 LEFT JOIN users ON orders.name = users.name
        WHERE NOT (NOT ((orders.age = '20') OR (NOT (orders.age = '20')))) AND NOT (users.age = '20')
           OR ((users.age = '20') OR (NOT (users.name = 'bob'))) AND
              NOT NOT (NOT ((NOT (orders.name = 'bob')) OR (orders.name = 'bob'))) AND NOT (users.age = '20')
           OR ((users.age = '20') AND (NOT (users.color = 'blue')))),
       (SELECT COUNT(*)
        FROM orders
                 LEFT JOIN users ON orders.name = users.name
        WHERE NOT (NOT ((orders.age @@@ '20') OR (NOT (orders.age @@@ '20')))) AND NOT (users.age @@@ '20')
           OR ((users.age @@@ '20') OR (NOT (users.name @@@ 'bob'))) AND
              NOT NOT (NOT ((NOT (orders.name @@@ 'bob')) OR (orders.name @@@ 'bob'))) AND NOT (users.age @@@ '20')
           OR ((users.age @@@ '20') AND (NOT (users.color @@@ 'blue'))));

---- idx=92 ----
---- connector=AND NOT ----
SELECT (SELECT COUNT(*)
        FROM orders
                 RIGHT JOIN users ON orders.name = users.name
        WHERE NOT ((NOT (orders.color = 'blue')) AND (NOT (orders.color = 'blue'))) AND NOT (users.age = '20')
           OR ((NOT (users.color = 'blue')) OR (users.name = 'bob')) AND
              NOT NOT ((NOT (orders.color = 'blue')) OR (NOT (orders.color = 'blue'))) AND NOT (users.age = '20')
           OR ((NOT (users.color = 'blue')) AND (users.color = 'blue'))),
       (SELECT COUNT(*)
        FROM orders
                 RIGHT JOIN users ON orders.name = users.name
        WHERE NOT ((NOT (orders.color @@@ 'blue')) AND (NOT (orders.color @@@ 'blue'))) AND NOT (users.age @@@ '20')
           OR ((NOT (users.color @@@ 'blue')) OR (users.name @@@ 'bob')) AND
              NOT NOT ((NOT (orders.color @@@ 'blue')) OR (NOT (orders.color @@@ 'blue'))) AND NOT (users.age @@@ '20')
           OR ((NOT (users.color @@@ 'blue')) AND (users.color @@@ 'blue')));

---- idx=74 ----
---- connector=AND NOT ----
SELECT (SELECT COUNT(*)
        FROM orders
                 JOIN users ON orders.name = users.name
        WHERE ((orders.age = '20') AND (orders.age = '20')) AND (orders.color = 'blue') AND NOT (users.age = '20')
           OR ((users.name = 'bob') OR (NOT (users.name = 'bob'))) AND NOT ((orders.age = '20') AND (orders.age = '20'))
           OR (orders.color = 'blue') AND NOT (users.age = '20')
           OR ((users.name = 'bob') AND (NOT (users.color = 'blue')))),
       (SELECT COUNT(*)
        FROM orders
                 JOIN users ON orders.name = users.name
        WHERE
            ((orders.age @@@ '20') AND (orders.age @@@ '20')) AND (orders.color @@@ 'blue') AND NOT (users.age @@@ '20')
           OR ((users.name @@@ 'bob') OR (NOT (users.name @@@ 'bob'))) AND
              NOT ((orders.age @@@ '20') AND (orders.age @@@ '20'))
           OR (orders.color @@@ 'blue') AND NOT (users.age @@@ '20')
           OR ((users.name @@@ 'bob') AND (NOT (users.color @@@ 'blue'))));

--
-- removing the `uses_tantivy_to_query` code so that we always push down quals if we can exposed
-- a bug where we'd confuse vars by their names if different tables in the query contain fields
-- with the same names.  This tests for that
--
SELECT (SELECT COUNT(*)
        FROM products
                 JOIN orders ON products.name = orders.name
        WHERE (NOT (products.id = '3'))
           OR ((products.name = 'bob') AND (orders.id = '3'))),
       (SELECT COUNT(*)
        FROM products
                 JOIN orders ON products.name = orders.name
        WHERE (NOT (products.id @@@ '3'))
           OR ((products.name @@@ 'bob') AND (orders.id @@@ '3')));
