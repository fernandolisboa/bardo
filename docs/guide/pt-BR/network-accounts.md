---
id: network-accounts
title: Contas de rede
group: publishing
place: accounts
tour: accounts
---

# Contas de rede

Uma conta de rede é a presença de um [canal](channels.md) numa rede: o @ dele, os padrões de que todo post ali parte e o formato em que os vídeos são renderizados. Contas do YouTube, do Instagram Reels e do TikTok também podem ser conectadas, para o Bardo enviar a elas e ler os números delas. As contas ficam em [Contas](bardo:go/accounts). [Mostre a tela para mim](bardo:tour/accounts).

<a id="channel"></a>
## Escolha o canal

A lista mostra seus canais; escolha um para ver as contas dele ao lado. O canal precisa ser criado antes em [Canais](bardo:go/channels).

<a id="accounts"></a>
## Uma conta por rede

Um canal tem no máximo uma conta em cada uma das cinco redes: YouTube, TikTok, Instagram Reels, X e Kick. Cada cartão mostra a rede, o @ (sem a @), se o preset de render é o padrão da rede ou ajustado, e o preset numa linha.

Os renders fazem um arquivo por conta, e a etapa [Publicação](bardo:go/projects/publish) escreve um post por conta.

<a id="metadata"></a>
## Padrões de metadados

**Editar** abre o formulário da conta. Os padrões de metadados dela são o ponto de partida de todo post nessa rede:

- **Idioma dos metadados**: o idioma em que o texto do post é escrito; **O mesmo do canal** acompanha o do canal.
- **Visibilidade**: público, não listado ou privado no YouTube; público ou privado no TikTok. Posts no Instagram, no X e na Kick são sempre públicos.
- **Tags**: as tags com que todo post começa, uma por linha ou separadas por vírgula, sem #. Redes que usam hashtags as recebem na legenda.
- **Rodapé da descrição**: links, créditos ou um convite para seguir, adicionados ao fim de toda descrição ou legenda.

O Claude escreve o título, a descrição e as tags de cada post a partir desses padrões, e você edita o resultado na etapa Publicação. Veja [Metadados e seus limites](uploading.md#metadata).

<a id="preset"></a>
## Preset de render

Cada rede renderiza no seu próprio preset: o quadro (16:9 ou 9:16), a resolução, o codec, o bitrate, a duração máxima que ela aceita e a loudness que ela busca. Os padrões estão em [Redes e presets](render.md#targets). Em **Editar**, mude qualquer um deles ou mantenha o **padrão da rede**; a linha embaixo dos campos mostra o preset resultante. Um preset alterado deixa o último render da rede desatualizado.

<a id="connect"></a>
## Conectar uma conta

Contas do YouTube, do Instagram Reels e do TikTok mostram a conexão no cartão. Conectar exige o seu próprio app nessa rede, salvo uma vez em [Configurações › Redes](app-credentials.md); o guia de cada rede mostra o caminho:

- O **YouTube** conecta pelo navegador: [Conectar o YouTube](connect-youtube.md).
- O **Instagram** conecta com um token que você cola do Explorador da Graph API da Meta: [Conectar o Instagram](connect-instagram.md).
- O **TikTok** conecta pelo navegador: [Conectar o TikTok](connect-tiktok.md).

X e Kick não conectam: os posts deles são [exportados](exporting.md) e postados à mão.

<a id="states"></a>
## Estados da conexão

| Estado | Significa | Você pode |
| --- | --- | --- |
| Não conectada | O Bardo não tem acesso | **Conectar** |
| Aguardando o navegador | O navegador está com a página de consentimento da rede aberta; o Bardo espera até cinco minutos | **Cancelar** |
| Conferindo o token | Instagram: o Bardo está trocando com a Meta o token colado | Esperar |
| Escolha uma conta | Instagram: o token alcança várias contas | **Conectar esta** na conta certa |
| Conectada como | O Bardo pode enviar e ler números como essa conta | **Verificar**, **Desconectar** |
| Reconexão necessária | A rede recusou renovar o acesso: ele foi revogado ou expirou | **Reconectar**, **Desconectar** |

Os tokens ficam no Gerenciador de Credenciais do Windows, por perfil e por conta; o banco de dados do Bardo guarda só o nome e o id da conta, os escopos concedidos, quando o acesso expira e a última renovação. Uma conta conectada precisa ser desconectada antes de ser removida.

<a id="add"></a>
## Adicionar uma rede

Embaixo dos cartões, **Adicionar (rede)** abre o formulário de cada rede em que o canal ainda não tem conta. Preencha o @, mude os padrões se quiser e escolha **Adicionar conta**. Para remover uma conta, abra o menu **⋯** no cartão dela; o Bardo pergunta antes.
