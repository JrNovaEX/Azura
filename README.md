<p align="center">
  <img src="crates/azura/assets/logo/azura.png" width="96" alt="Azura logo">
</p>

<h1 align="center">Azura</h1>

<p align="center">
  Start the apps you use every day in one click, in the right order.<br>
  Free, open source, and private.
</p>

<p align="center">
  <a href="https://github.com/JrNovaEX/Azura/releases/latest"><b>Download for Linux</b></a>
  ·
  <a href="#frequently-asked-questions">FAQ</a>
  ·
  <a href="docs/DEVELOPMENT.md">For developers</a>
</p>

![Azura: your whole setup in one click](docs/media/01-hero.png)

## The problem

Most mornings look the same. You open your VPN and wait for it to connect.
You open a terminal and start a tool. Then your chat app, your AI assistant,
your browser. Some of them have to wait for the one before. You do it by hand,
in the same order, every single day.

![The same routine, by hand, every day](docs/media/02-problem.png)

You could put all of it in your startup list. But then everything launches the
moment you sign in, at the same time, and your computer stays slow and noisy
until it settles down. Often before you even need those apps.

## What Azura does

Azura lets you save that routine once, as a **session**, and start it with
one click whenever you are ready.

- Your apps open **one after another**, in the order you chose.
- You can add a **short pause** after any step. For example, give your VPN
  5 seconds to connect before anything else opens.
- Your computer still **starts light**. Azura opens when you sign in, waits
  quietly, and does nothing until you press Run.

![Your computer starts light. Your apps start when you say so.](docs/media/03-boot.png)

## How it works

1. **Create a session.** Give it a name, like "Morning" or "Deep work", and
   pick an icon.
2. **Add what you need.** Choose apps from the ones installed on your
   computer, add a terminal command, or add a link, file or folder.
3. **Press Run.** Azura opens everything in order and shows you how it went.

![VPN first. Then everything else.](docs/media/04-order.png)

## Features

- **Real apps only.** Azura shows the apps that are actually installed on
  your computer, with their real names and icons.
- **More than apps.** Run a command in a terminal window (or quietly in the
  background), or open a website, a file or a folder.
- **Order and timing.** Drag items to reorder them. Add a pause after any
  item.
- **Honest progress.** You see what launched, what failed and why. If an app
  was uninstalled, Azura tells you instead of pretending.
- **Several sessions.** Keep one for mornings, one for focused work, one for
  the evening.
- **English and Persian**, with a full right-to-left Persian interface.
- **Light and dark themes**, following your system by default.
- **Stays up to date.** Azura notices a new version, checks it against its
  published checksum, and tells you how to install it.

![Pick from what you actually have](docs/media/05-apps.png)

![English and Persian](docs/media/06-languages.png)

## Download

Azura is for Linux. Get the latest version from the
[Releases page](https://github.com/JrNovaEX/Azura/releases/latest).

| System | File |
| ------ | ---- |
| Debian, Ubuntu, Mint, and other `.deb` distros (x86_64) | `azura_<version>_amd64.deb` |
| Any Linux (x86_64) | `azura-<version>-linux-x64.tar.gz` |

### The .deb

```sh
sudo apt install ./azura_<version>_amd64.deb
```

Azura shows up in your app menu, and you can remove it any time with
`sudo apt remove azura`. Removing it leaves your sessions alone.

### Tarball

Unpack it and run `./azura`. This is the file Azura itself downloads when it
finds a new version.

## Your privacy

- Azura works **without an internet connection**. The only thing it ever
  asks the internet is whether a new version of Azura is out (see below).
- There is **no account**, no tracking and no analytics.
- Your sessions are saved in one small file on your own computer.
- Nothing runs until you press Run.

## Frequently asked questions

**Is Azura a startup manager?**
No. A startup manager opens apps automatically when you sign in. Azura does
the opposite: it keeps your sign-in light and waits for you to decide when your
apps should start.

**Will it slow down my computer?**
No. While it waits, Azura uses no processor time and very little memory. It
does not run anything in the background.

**Does Azura know when my VPN has connected?**
No, and it does not pretend to. It opens each app and then waits the number of
seconds you set before opening the next one. Choose a pause that fits your VPN.

**Can it run terminal commands?**
Yes. Add a **Command**, type it exactly as you would in a terminal, and choose
whether it opens in a terminal window or runs quietly in the background.

**How does Azura update itself?**
Once a day, Azura asks GitHub whether a new version has been released.
Nothing about you or your sessions is sent. If there is a new one, Azura
downloads it in the background and checks it against the checksum published
with the release, then shows you where the file is.

Azura never installs anything by itself. On Linux there is no single way to
install a program, so that last step is yours: install the `.deb`, or unpack
the tarball. You can turn automatic checks off in **Settings**, and check by
hand with **Check now**.

**Can I stop Azura from opening when I sign in?**
Yes. Open **Settings** and turn off **Open when you sign in**.

**Something did not open. What now?**
Azura shows the reason next to the item, in the words your system gave. If
an app was moved or uninstalled, remove it and add it again.

## Help make Azura better

Azura is free and open source. Bug reports, ideas and translations are all
welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) to get started, and
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for how it is built.

## License

Azura is released under the [MIT License](LICENSE). It includes icons from
[Lucide](https://lucide.dev) and the
[Vazirmatn](https://github.com/rastikerdar/vazirmatn) typeface. See
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md#licenses-of-bundled-material) for
details.

---

<div dir="rtl">

### آزورا به فارسی

آزورا یه ابزار کوچیک و آفلاینه. برنامه‌ها، دستورها و لینک‌هایی که هر روز باز
می‌کنی رو توی «روتین»ها بچین تا آزورا اون‌ها رو به ترتیب و با فاصله‌ی زمانی
دلخواهت اجرا کنه. فقط برنامه‌هایی رو می‌بینی که واقعا روی سیستمت نصب‌ان، با
اسم و نماد واقعی خودشون. بدون حساب کاربری، بدون اینترنت و بدون ردیابی

آزورا فقط روی لینوکس کار می‌کنه. برای نصب، بسته‌ی deb رو از
[صفحه‌ی انتشار](https://github.com/JrNovaEX/Azura/releases/latest) بگیر و
اجرا کن:

```sh
sudo apt install ./azura_<version>_amd64.deb
```

</div>
