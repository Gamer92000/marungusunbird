function augment(channel) {
  fetch(`/augmentations/${channel}/augment`, {
    method: 'POST',
    body: JSON.stringify({
      first: '╓─ ',
      middle: '╟─ ',
      last: '╙─ '
    }),
    headers: {
      'Content-Type': 'application/json'
    }
  }).then(response => response.text()).then(text => {
    if (text != "Success") {
      alert(text);
    }
    location.reload();
  });
}

function unaugment(channel) {
  fetch(`/augmentations/${channel}/remove`, {
    method: 'POST',
  }).then(response => response.text()).then(text => {
    if (text != "Success") {
      alert(text);
    }
    location.reload();
  });
}