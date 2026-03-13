# Uploads API

Upload and retrieve files via the API.

## Upload a file

```
POST /api/v1/uploads
Content-Type: multipart/form-data
```

```sh
curl -X POST \
  -H "Authorization: Bearer $TOKEN" \
  -F "file=@photo.jpg" \
  http://localhost:3000/api/v1/uploads
```

Response:

```json
{
  "hash": "a1b2c3d4e5f67890abcdef1234567890abcdef1234567890abcdef1234567890",
  "filename": "photo.jpg",
  "mime": "image/jpeg",
  "size": 245760
}
```

Use the `hash` value when referencing this upload in content entries.

If the same file is uploaded again (identical content, same SHA-256 hash), the existing file is reused. Only one copy is stored on disk.

## Download a file

```
GET /api/v1/uploads/:hash
```

```sh
curl -H "Authorization: Bearer $TOKEN" \
  http://localhost:3000/api/v1/uploads/a1b2c3d4e5f67890... \
  -o photo.jpg
```

Returns the file with the correct `Content-Type` header.

Returns `404` if no upload with that hash exists.

## Public file access

Uploads are also available without authentication at:

```
/uploads/file/:hash/:filename
```

This public URL is used by the web UI to display uploaded images and link to files. The filename in the URL is cosmetic -- the hash is what identifies the file.
