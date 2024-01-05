async function augment(event) {
  event.preventDefault();
  let form = event.target;
  await fetch(`/augmentation/${form.channel.value}/augment`, {
    method: 'POST',
    body: JSON.stringify({
      first: form.first_prefix.value,
      middle: form.middle_prefix.value,
      last: form.last_prefix.value,
    }),
    headers: {
      'Content-Type': 'application/json'
    }
  }).then(response => response.text()).then(text => {
    if (text != "Success") {
      alert(text);
    }
    window.location.reload();
  });
}

async function change_prefix(event) {
  event.preventDefault();
  let form = event.target;
  await fetch(`/augmentation/${form.channel.value}/change_prefix`, {
    method: 'POST',
    body: JSON.stringify({
      first: form.first_prefix.value,
      middle: form.middle_prefix.value,
      last: form.last_prefix.value,
    }),
    headers: {
      'Content-Type': 'application/json'
    }
  }).then(response => response.text()).then(text => {
    if (text != "Success") {
      alert(text);
    }
    window.location.reload();
  });
}

async function update_augmentation(event) {
  event.preventDefault();
  console.log(event);
  let type = event.submitter.name;
  if (type == "update") {
    await change_prefix(event);
  }
  else if (type == "abridge") {
    await abridge_augmentation(event);
  }
}

async function abridge_augmentation(event) {
  event.preventDefault();
  let channel = event.target.channel.value;
  await fetch(`/augmentation/${channel}/abridge`, {
    method: 'POST',
  }).then(response => response.text()).then(text => {
    if (text != "Success") {
      alert(text);
    }
    window.location.reload();
  });
}