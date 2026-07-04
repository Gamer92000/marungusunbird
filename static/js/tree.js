// Repeat/fill spacers ([*spacer]<unit>) tile their unit string across the whole
// row. The final width is only known in the browser, so measure it here and
// repeat the unit enough times to cover the row, re-running on resize and after
// every live tree update.
function fillRepeatSpacers(root) {
  const scope = root || document;
  const fills = scope.querySelectorAll(".tree_item.spacer.repeat .fill");
  fills.forEach((el) => {
    const unit = el.dataset.unit;
    if (!unit) return;
    const width = el.clientWidth;
    if (!width) return;
    // Measure one unit's rendered width using a detached clone.
    const probe = el.cloneNode(false);
    probe.style.position = "absolute";
    probe.style.visibility = "hidden";
    probe.style.width = "auto";
    probe.style.whiteSpace = "pre";
    probe.textContent = unit;
    el.parentNode.appendChild(probe);
    const unitWidth = probe.getBoundingClientRect().width || 1;
    probe.remove();
    const count = Math.ceil(width / unitWidth) + 2;
    el.textContent = unit.repeat(count);
  });
}

window.fillRepeatSpacers = fillRepeatSpacers;
window.addEventListener("load", () => fillRepeatSpacers());
window.addEventListener("resize", () => fillRepeatSpacers());

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