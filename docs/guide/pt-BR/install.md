---
id: install
title: Instalar o Bardo
group: getting-started
---

# Instalar o Bardo

O Bardo se instala como qualquer app do Windows, só para o seu usuário do Windows: não precisa de direitos de administrador, e nada do que ele instala chega aos outros usuários do computador.

<a id="download"></a>
## Download

Cada versão do Bardo é uma release no [GitHub](https://github.com/fernandolisboa/bardo/releases), com um arquivo para baixar: `Bardo-Setup-<versão>-x64.exe`. O Bardo roda no Windows 10 e 11 de 64 bits.

Ao lado dele, um arquivo `.sha256` traz o checksum SHA-256 do instalador. Para conferir o download, rode `Get-FileHash Bardo-Setup-<versão>-x64.exe` no PowerShell e compare os dois.

<a id="smartscreen"></a>
## "O Windows protegeu o computador"

O instalador ainda não é assinado com um certificado de assinatura de código, então, na primeira vez que você abrir, o Windows SmartScreen pode barrar com **O Windows protegeu o computador**. Selecione **Mais informações**, confira se o arquivo é o que você baixou e selecione **Executar assim mesmo**.

<a id="setup"></a>
## Instalação

O instalador fala português ou inglês, conforme o Windows. Ele pergunta:

- **Onde**: por padrão, `%LOCALAPPDATA%\Programs\Bardo`.
- **Um atalho na área de trabalho**, se você quiser. O menu Iniciar sempre ganha um.
- **Publicar mesmo com o Bardo fechado?** Só na primeira instalação, e desmarcado a menos que você marque. Marcado, o Bardo liga o agente em segundo plano na próxima vez que abrir, como faz **Publicar com o Bardo fechado**. Dá para mudar a qualquer momento em [Configurações › Publicação](background-agent.md#turn-on).
- **Importar configurações de um arquivo?** Só na primeira instalação. Escolha um [arquivo de configurações](backup.md#settings-file) exportado de outra cópia do Bardo, ou deixe em branco. Na próxima vez que abrir, o Bardo pede a senha do arquivo e deixa você marcar o que importar.

O FFmpeg, com que o Bardo lê, pré-visualiza e renderiza vídeo, vem dentro do instalador: não há mais nada para instalar. Em seguida vêm suas chaves de API: veja [Suas chaves de API](api-keys.md).

<a id="update"></a>
## Atualizar

O Bardo avisa quando sai uma versão mais nova e a instala para você depois da sua revisão: veja [Atualizações](about.md#updates).

Para atualizar manualmente, rode o instalador da versão mais nova por cima da que você tem. Ele mantém a pasta, suas configurações e sua escolha sobre o agente em segundo plano, e fecha o Bardo e o agente antes, se estiverem rodando. O Bardo mostra a versão em [Configurações › Sobre](about.md#version).

<a id="uninstall"></a>
## Desinstalar

Desinstale o Bardo em Configurações do Windows › Aplicativos › Aplicativos instalados. Feche o Bardo antes; o desinstalador pede isso se ele ainda estiver aberto. Ele desliga o agente em segundo plano e o remove do Windows, depois remove o programa.

Seus projetos, exportações, configurações, backups automáticos e as chaves no Gerenciador de Credenciais do Windows ficam onde estão, então instalar o Bardo de novo os encontra. Veja [Onde ficam seus dados e chaves](data-and-keys.md).
