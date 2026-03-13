# Content API

Full CRUD for content entries. Endpoints differ slightly for [collection vs single](./single-vs-collection.md) schemas.

## Collection endpoints

### List entries

```
GET /api/v1/content/:schema_slug
```

```sh
curl -H "Authorization: Bearer $TOKEN" \
  http://localhost:3000/api/v1/content/blog-posts
```

Response:

```json
[
  {
    "id": "my-first-post",
    "data": {
      "title": "My First Post",
      "body": "Hello world",
      "published": true
    }
  }
]
```

### Get an entry

```
GET /api/v1/content/:schema_slug/:entry_id
```

```sh
curl -H "Authorization: Bearer $TOKEN" \
  http://localhost:3000/api/v1/content/blog-posts/my-first-post
```

Response -- the entry data directly (no wrapper):

```json
{
  "title": "My First Post",
  "body": "Hello world",
  "published": true
}
```

Returns `404` if the entry does not exist.

### Create an entry

```
POST /api/v1/content/:schema_slug
Content-Type: application/json
```

```sh
curl -X POST \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"title": "New Post", "body": "Content here"}' \
  http://localhost:3000/api/v1/content/blog-posts
```

Response (`201 Created`):

```json
{
  "id": "new-post"
}
```

The entry ID is generated from the content (see [entry ID generation](./schemas.md#entry-id-generation)).

Validation errors return `400`:

```json
{
  "errors": ["title: \"title\" is a required property"]
}
```

### Update an entry

```
PUT /api/v1/content/:schema_slug/:entry_id
Content-Type: application/json
```

```sh
curl -X PUT \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"title": "Updated Post", "body": "New content", "published": true}' \
  http://localhost:3000/api/v1/content/blog-posts/new-post
```

Returns `200 OK` on success.

### Delete an entry

```
DELETE /api/v1/content/:schema_slug/:entry_id
```

```sh
curl -X DELETE \
  -H "Authorization: Bearer $TOKEN" \
  http://localhost:3000/api/v1/content/blog-posts/new-post
```

Returns `204 No Content` on success.

## Single endpoints

For schemas with `kind: "single"`, use the `/single` endpoints instead:

### Get

```
GET /api/v1/content/:schema_slug/single
```

```sh
curl -H "Authorization: Bearer $TOKEN" \
  http://localhost:3000/api/v1/content/site-settings/single
```

### Create or update

```
PUT /api/v1/content/:schema_slug/single
Content-Type: application/json
```

```sh
curl -X PUT \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"site_name": "My Site", "tagline": "A great site"}' \
  http://localhost:3000/api/v1/content/site-settings/single
```

### Delete

```
DELETE /api/v1/content/:schema_slug/single
```

## Working with uploads in content

When creating or updating content that includes upload fields, use the upload hash reference format:

```json
{
  "title": "Post with Image",
  "cover": {
    "hash": "a1b2c3d4e5f6...",
    "filename": "photo.jpg",
    "mime": "image/jpeg"
  }
}
```

Upload the file first via the [Uploads API](./api-uploads.md), then use the returned hash in your content.
