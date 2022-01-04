<script>
  let msg = "hello";
</script>

<div class="container">
  <div>Message:</div>
  <div class="message">{msg}</div>
</div>

<button on:click={() => (msg = msg + " hello")}>Change Message</button>

<style>
  .container {
    margin: 8px;
  }
  .message {
    text-decoration: underline;
  }
</style>