require "dotenv/load"
require "aws-sdk-s3"

bucket = ARGV.fetch(0)
access_key_id = ENV.fetch("CLOUDFLARE_R2_ACCESS_KEY_ID")
secret_access_key = ENV.fetch("CLOUDFLARE_R2_SECRET_ACCESS_KEY")
endpoint = ENV.fetch('CLOUDFLARE_R2_ENDPOINT')

client = Aws::S3::Client.new(
  access_key_id:,
  secret_access_key:,
  region: "auto",
  endpoint:,
  force_path_style: true
)

continuation_token = nil

loop do
  resp = client.list_objects_v2(
    bucket: bucket,
    continuation_token: continuation_token,
    max_keys: 1000
  )

  break if resp.contents.empty?

  resp.contents.each_slice(1000) do |slice|
    client.delete_objects(
      bucket: bucket,
      delete: {
        objects: slice.map { |obj| { key: obj.key } },
        quiet: true
      }
    )
  end

  break unless resp.is_truncated
  continuation_token = resp.next_continuation_token
end
