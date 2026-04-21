// Central collection of GraphQL query/mutation strings used by the CLI.
// Kept here (rather than inline in handlers) so that adding a new command only
// means adding the constant and wiring it up.

pub const CURRENT_USER_QUERY: &str = r#"
query getCurrentUser {
  currentUser {
    id
    name
    email
    emailVerified
    avatarUrl
    hasPassword
  }
}
"#;

pub const LIST_WORKSPACES_QUERY: &str = r#"
query getWorkspaces {
  workspaces {
    id
    initialized
    team
    public
    role
    createdAt
    enableAi
    enableSharing
    enableUrlPreview
    enableDocEmbedding
    memberCount
    owner {
      id
      name
      email
    }
  }
}
"#;

pub const GET_WORKSPACE_QUERY: &str = r#"
query getWorkspace($id: String!) {
  workspace(id: $id) {
    id
    initialized
    team
    public
    role
    createdAt
    enableAi
    enableSharing
    enableUrlPreview
    enableDocEmbedding
    memberCount
    inviteLink {
      link
      expireTime
    }
    owner {
      id
      name
      email
      avatarUrl
    }
    quota {
      name
      blobLimit
      storageQuota
      usedStorageQuota
      historyPeriod
      memberLimit
      memberCount
      overcapacityMemberCount
      humanReadable {
        name
        blobLimit
        storageQuota
        historyPeriod
        memberLimit
        memberCount
      }
    }
  }
}
"#;

pub const CREATE_WORKSPACE_QUERY: &str = r#"
mutation createWorkspace {
  createWorkspace {
    id
    public
    createdAt
    initialized
  }
}
"#;

pub const CREATE_WORKSPACE_WITH_INIT_QUERY: &str = r#"
mutation createWorkspace($init: Upload!) {
  createWorkspace(init: $init) {
    id
    public
    createdAt
    initialized
  }
}
"#;

pub const UPDATE_WORKSPACE_QUERY: &str = r#"
mutation updateWorkspace($input: UpdateWorkspaceInput!) {
  updateWorkspace(input: $input) {
    id
    public
    enableAi
    enableSharing
    enableUrlPreview
    enableDocEmbedding
  }
}
"#;

pub const DELETE_WORKSPACE_QUERY: &str = r#"
mutation deleteWorkspace($id: String!) {
  deleteWorkspace(id: $id)
}
"#;

pub const LIST_DOCS_QUERY: &str = r#"
query listDocs($workspaceId: String!, $pagination: PaginationInput!) {
  workspace(id: $workspaceId) {
    docs(pagination: $pagination) {
      totalCount
      pageInfo {
        startCursor
        endCursor
        hasNextPage
        hasPreviousPage
      }
      edges {
        cursor
        node {
          id
          title
          public
          mode
          defaultRole
          summary
          createdAt
          updatedAt
          creatorId
          lastUpdaterId
          workspaceId
        }
      }
    }
  }
}
"#;

pub const LIST_RECENT_DOCS_QUERY: &str = r#"
query listRecentDocs($workspaceId: String!, $pagination: PaginationInput!) {
  workspace(id: $workspaceId) {
    recentlyUpdatedDocs(pagination: $pagination) {
      totalCount
      pageInfo {
        startCursor
        endCursor
        hasNextPage
        hasPreviousPage
      }
      edges {
        cursor
        node {
          id
          title
          summary
          mode
          public
          createdAt
          updatedAt
        }
      }
    }
  }
}
"#;

pub const LIST_PUBLIC_DOCS_QUERY: &str = r#"
query listPublicDocs($workspaceId: String!) {
  workspace(id: $workspaceId) {
    publicDocs {
      id
      title
      summary
      mode
      public
      createdAt
      updatedAt
    }
  }
}
"#;

pub const GET_DOC_QUERY: &str = r#"
query getDoc($workspaceId: String!, $docId: String!) {
  workspace(id: $workspaceId) {
    doc(docId: $docId) {
      id
      title
      public
      mode
      defaultRole
      summary
      createdAt
      updatedAt
      creatorId
      lastUpdaterId
      workspaceId
      createdBy {
        id
        name
        avatarUrl
      }
      lastUpdatedBy {
        id
        name
        avatarUrl
      }
      meta {
        createdAt
        updatedAt
        createdBy {
          name
          avatarUrl
        }
        updatedBy {
          name
          avatarUrl
        }
      }
    }
  }
}
"#;

pub const GET_DOC_ANALYTICS_QUERY: &str = r#"
query getDocAnalytics($workspaceId: String!, $docId: String!, $input: DocPageAnalyticsInput) {
  workspace(id: $workspaceId) {
    doc(docId: $docId) {
      id
      title
      analytics(input: $input) {
        generatedAt
        window {
          from
          to
        }
        summary {
          totalViews
          uniqueViews
          guestViews
        }
        series {
          date
          totalViews
          uniqueViews
          guestViews
        }
      }
    }
  }
}
"#;

pub const SEARCH_DOCS_QUERY: &str = r#"
query searchDocs($id: String!, $input: SearchDocsInput!) {
  workspace(id: $id) {
    searchDocs(input: $input) {
      docId
      title
      blockId
      highlight
      createdAt
      updatedAt
      createdByUser {
        id
        name
        avatarUrl
      }
      updatedByUser {
        id
        name
        avatarUrl
      }
    }
  }
}
"#;

pub const PUBLISH_DOC_QUERY: &str = r#"
mutation publishDoc($workspaceId: String!, $docId: String!, $mode: PublicDocMode) {
  publishDoc(workspaceId: $workspaceId, docId: $docId, mode: $mode) {
    id
    public
    mode
  }
}
"#;

pub const REVOKE_PUBLIC_DOC_QUERY: &str = r#"
mutation revokePublicDoc($workspaceId: String!, $docId: String!) {
  revokePublicDoc(workspaceId: $workspaceId, docId: $docId) {
    id
    public
    mode
  }
}
"#;

pub const GRANT_DOC_USER_ROLES_QUERY: &str = r#"
mutation grantDocUserRoles($input: GrantDocUserRolesInput!) {
  grantDocUserRoles(input: $input)
}
"#;

pub const UPDATE_DOC_USER_ROLE_QUERY: &str = r#"
mutation updateDocUserRole($input: UpdateDocUserRoleInput!) {
  updateDocUserRole(input: $input)
}
"#;

pub const REVOKE_DOC_USER_ROLES_QUERY: &str = r#"
mutation revokeDocUserRoles($input: RevokeDocUserRoleInput!) {
  revokeDocUserRoles(input: $input)
}
"#;

pub const UPDATE_DOC_DEFAULT_ROLE_QUERY: &str = r#"
mutation updateDocDefaultRole($input: UpdateDocDefaultRoleInput!) {
  updateDocDefaultRole(input: $input)
}
"#;

pub const LIST_BLOBS_QUERY: &str = r#"
query listBlobs($workspaceId: String!) {
  workspace(id: $workspaceId) {
    blobs {
      key
      size
      mime
      createdAt
    }
  }
}
"#;

pub const BLOB_USAGE_QUERY: &str = r#"
query blobUsage($workspaceId: String!) {
  workspace(id: $workspaceId) {
    blobsSize
    quota {
      name
      blobLimit
      storageQuota
      usedStorageQuota
      humanReadable {
        name
        blobLimit
        storageQuota
      }
    }
  }
}
"#;

pub const SET_BLOB_QUERY: &str = r#"
mutation setBlob($workspaceId: String!, $blob: Upload!) {
  setBlob(workspaceId: $workspaceId, blob: $blob)
}
"#;

pub const CREATE_BLOB_UPLOAD_QUERY: &str = r#"
mutation createBlobUpload($workspaceId: String!, $key: String!, $mime: String!, $size: Int!) {
  createBlobUpload(workspaceId: $workspaceId, key: $key, mime: $mime, size: $size) {
    method
    uploadId
    uploadUrl
    partSize
    blobKey
    alreadyUploaded
    expiresAt
    headers
    uploadedParts {
      partNumber
      etag
    }
  }
}
"#;

pub const BLOB_UPLOAD_PART_URL_QUERY: &str = r#"
query blobUploadPartUrl($workspaceId: String!, $key: String!, $uploadId: String!, $partNumber: Int!) {
  workspace(id: $workspaceId) {
    blobUploadPartUrl(key: $key, uploadId: $uploadId, partNumber: $partNumber) {
      uploadUrl
      headers
      expiresAt
    }
  }
}
"#;

pub const COMPLETE_BLOB_UPLOAD_QUERY: &str = r#"
mutation completeBlobUpload($workspaceId: String!, $key: String!, $uploadId: String, $parts: [BlobUploadPartInput!]) {
  completeBlobUpload(workspaceId: $workspaceId, key: $key, uploadId: $uploadId, parts: $parts)
}
"#;

pub const ABORT_BLOB_UPLOAD_QUERY: &str = r#"
mutation abortBlobUpload($workspaceId: String!, $key: String!, $uploadId: String!) {
  abortBlobUpload(workspaceId: $workspaceId, key: $key, uploadId: $uploadId)
}
"#;

pub const DELETE_BLOB_QUERY: &str = r#"
mutation deleteBlob($workspaceId: String!, $key: String!, $permanently: Boolean) {
  deleteBlob(workspaceId: $workspaceId, key: $key, permanently: $permanently)
}
"#;

pub const RELEASE_DELETED_BLOBS_QUERY: &str = r#"
mutation releaseDeletedBlobs($workspaceId: String!) {
  releaseDeletedBlobs(workspaceId: $workspaceId)
}
"#;

pub const GENERATE_ACCESS_TOKEN_QUERY: &str = r#"
mutation generateUserAccessToken($input: GenerateAccessTokenInput!) {
  generateUserAccessToken(input: $input) {
    id
    name
    token
    createdAt
    expiresAt
  }
}
"#;

pub const REVOKE_ACCESS_TOKEN_QUERY: &str = r#"
mutation revokeUserAccessToken($id: String!) {
  revokeUserAccessToken(id: $id)
}
"#;

pub const LIST_ACCESS_TOKENS_QUERY: &str = r#"
query listAccessTokens {
  currentUser {
    revealedAccessTokens {
      id
      name
      createdAt
      expiresAt
    }
  }
}
"#;

pub const SEND_VERIFY_EMAIL_QUERY: &str = r#"
mutation sendVerifyEmail($callbackUrl: String!) {
  sendVerifyEmail(callbackUrl: $callbackUrl)
}
"#;

pub const VERIFY_EMAIL_QUERY: &str = r#"
mutation verifyEmail($token: String!) {
  verifyEmail(token: $token)
}
"#;
